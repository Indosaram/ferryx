// allow: SIZE_OK — core client session state machine and network driver
use std::{
    collections::VecDeque,
    io,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ferryx_rd_net::{
    DatagramCipher, DatagramError, Direction, PskIdentity, TlsPskClient, TlsPskError, TlsPskStream,
};
use ferryx_rd_proto::{
    AudioFragment, BitrateAdjust, Capabilities, ClipboardSyncDirection, ClipboardSyncOrigin,
    ClipboardSyncUpdate, ControlMessage, CursorUpdate, FrameChunk, FrameHeader, Handshake,
    HostDisplay, HostPolicy, InputEvent, PacketHeader, PacketType, PairingGrant, PairingReject,
    PairingRejectReason, PairingRequest, StreamOptionsActive, StreamOptionsRequest, WireCodec,
    PROTOCOL_VERSION,
};
use rand::{rngs::OsRng, RngCore};
use thiserror::Error;

use crate::{
    AssembledFrame, AudioFragmentReassembler, CursorState, FrameAssembler, PairingRecord,
    PairingStore, PairingStoreError, PlatformClipboard,
};

#[cfg(test)]
#[path = "tcp_write_tests.rs"]
mod tcp_write_tests;

#[path = "receiver_trace.rs"]
mod receiver_trace;
pub use receiver_trace::*;

#[path = "session_recovery.rs"]
mod recovery;

#[path = "session_parity.rs"]
mod parity;
pub use parity::{ReconnectState, SessionInfo, SessionStats};

pub const DEFAULT_TCP_PORT: u16 = 19_730;
pub const DEFAULT_UDP_PORT: u16 = 19_731;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const HANDSHAKE_ACK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    AwaitingPairing,
    AwaitingHandshakeAck,
    Ready,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub host: String,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub client_name: String,
    pub capabilities: Capabilities,
    pub pairing_store_path: Option<PathBuf>,
    pub connect_timeout: Duration,
    pub handshake_ack_timeout: Duration,
}

impl SessionConfig {
    pub fn implemented_capabilities() -> Capabilities {
        Capabilities::STREAM_CONFIGURATION
            | Capabilities::TEXT_CLIPBOARD_SYNC
            | Capabilities::AUTHENTICATED_UDP_REGISTRATION
            | Capabilities::STREAM_OPTIONS
            | Capabilities::HOST_STATUS
            | Capabilities::SESSION_ENDED
            | Capabilities::GUEST_PERMISSIONS
            | Capabilities::SECURE_ATTENTION
            | Capabilities::AUDIO_SOURCE_TIME
            | Capabilities::AUDIO_OPUS
            | Capabilities::RTT_PROBE
            | Capabilities::FEC_NACK
            | Capabilities::CLOCK_CORRELATION
            | Capabilities::CURSOR_METADATA
            | Capabilities::INPUT_ECHO
            | Capabilities::CLIPBOARD_IMAGE
            | Capabilities::PAIRING_TRUST
            | Capabilities::UDP_INPUT
            | Capabilities::CURSOR_SHAPE
    }

    pub fn direct(host: impl Into<String>, client_name: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            tcp_port: DEFAULT_TCP_PORT,
            udp_port: DEFAULT_UDP_PORT,
            client_name: client_name.into(),
            capabilities: Self::implemented_capabilities(),
            pairing_store_path: None,
            connect_timeout: CONNECT_TIMEOUT,
            handshake_ack_timeout: HANDSHAKE_ACK_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReadySession {
    pub pairing: PairingRecord,
    pub server: Handshake,
    pub session_salt: [u8; 16],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamState {
    pub host_supports_stream_options: bool,
    pub active: Option<StreamOptionsActive>,
    pub displays: Vec<HostDisplay>,
    pub policy: Option<HostPolicy>,
    pub last_reject: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    Frame(AssembledFrame),
    Audio(Vec<u8>),
    Cursor(CursorState),
    Clipboard(String),
    StreamConfig(ControlMessage),
    InputAck {
        sequence: u32,
        success: bool,
        error_code: u8,
    },
    Ping,
    Ignored,
}

/// Whether a session failure is worth re-handshaking for.
///
/// The Windows host service replaces its session worker whenever the console
/// switches desktop (entering or leaving the logon, lock or UAC secure desktop).
/// That terminates the worker holding this client's control channel and media
/// session, so a connected client goes silent even though the host is healthy
/// and already serving a new worker. Those failures are recoverable by
/// reconnecting with the stored pairing; credential and addressing failures are
/// not, and retrying them would spin.
pub fn should_reconnect(error: &SessionError) -> bool {
    matches!(
        error,
        SessionError::NotReady
            | SessionError::TcpRuntimeStopped
            | SessionError::TcpRuntimePanicked(_)
            | SessionError::HandshakeAckTimeout
    ) || matches!(error, SessionError::Io(e) | SessionError::Tls(TlsPskError::Io(e)) if matches!(e.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted | io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut))
        // OpenSSL can report an unclean EOF as SSL_ERROR_SYSCALL without an
        // OS error. The TLS frame reader wraps it in io::ErrorKind::Other.
        // This is an established-stream failure, not a PSK handshake rejection
        // (which remains TlsPskError::Handshake and must never be retried).
        || matches!(error, SessionError::Tls(TlsPskError::Io(e)) if e.kind() == io::ErrorKind::Other)
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session is not ready")]
    NotReady,
    #[error("pairing rejected: {0:?}")]
    PairingRejected(PairingRejectReason),
    #[error("pairing record {0} was not found")]
    PairingNotFound(String),
    #[error("expected {expected:?}, received {actual:?}")]
    UnexpectedPacket {
        expected: PacketType,
        actual: PacketType,
    },
    #[error("handshake acknowledgement timed out")]
    HandshakeAckTimeout,
    #[error("session lock was poisoned")]
    Poisoned,
    #[error("address resolution returned no endpoints")]
    NoAddress,
    #[error("TCP runtime worker panicked: {0}")]
    TcpRuntimePanicked(String),
    #[error("TCP runtime stopped")]
    TcpRuntimeStopped,
    #[error("TCP runtime already owns the connection; stop it before reconnecting")]
    TcpRuntimeAlreadyRunning,
    #[error("protocol error: {0}")]
    Protocol(#[from] ferryx_rd_proto::CodecError),
    #[error("TLS transport error: {0}")]
    Tls(#[from] TlsPskError),
    #[error("UDP transport error: {0}")]
    Datagram(#[from] DatagramError),
    #[error("pairing store error: {0}")]
    PairingStore(#[from] PairingStoreError),
    #[error("I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("host lacks authenticated UDP registration capability")]
    MissingAuthenticatedRegistration,
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation is unsupported or was not negotiated")]
    Unsupported,
    #[error("operation denied by effective guest permissions")]
    PermissionDenied,
    #[error("invalid clipboard image transfer")]
    InvalidClipboardImage,
}

struct SessionStateInner {
    state: SessionState,
    next_tcp_sequence: u32,
    next_udp_sequence: u32,
    pairing: Option<PairingRecord>,
    server: Option<Handshake>,
    session_salt: Option<[u8; 16]>,
    frames: FrameAssembler,
    abr: Option<crate::AbrController>,
    abr_bounds: Option<(i32, i32)>,
    abr_tick: Option<std::time::Instant>,
    recovery_feedback_at: Option<std::time::Instant>,
    receiver: crate::receiver_stats::ReceiverStats,
    audio: AudioFragmentReassembler,
    audio_playback: crate::audio_playback::AudioPlayback,
    cursor: CursorState,
    cursor_shapes: crate::CursorShapeCache,
    stream_state: StreamState,
    parity: parity::ParityState,
}

impl SessionStateInner {
    fn confirm_abr(&mut self, bitrate: u32) {
        let bitrate = bitrate.min(i32::MAX as u32) as i32;
        if bitrate <= 0 {
            return;
        }
        let (min, max) = self.abr_bounds.unwrap_or((bitrate.min(1_000_000), bitrate));
        self.abr = Some(crate::AbrController::new(bitrate.clamp(min, max), min, max));
        self.abr_tick = None;
    }
}

impl Default for SessionStateInner {
    fn default() -> Self {
        Self {
            state: SessionState::Disconnected,
            next_tcp_sequence: 0,
            next_udp_sequence: 0,
            pairing: None,
            server: None,
            session_salt: None,
            frames: FrameAssembler::default(),
            abr: None,
            abr_bounds: None,
            abr_tick: None,
            recovery_feedback_at: None,
            receiver: crate::receiver_stats::ReceiverStats::default(),
            audio: AudioFragmentReassembler::default(),
            audio_playback: crate::audio_playback::AudioPlayback::default(),
            cursor: CursorState::default(),
            cursor_shapes: crate::CursorShapeCache::default(),
            stream_state: StreamState::default(),
            parity: parity::ParityState::default(),
        }
    }
}

/// Thread-safe client session orchestrator. Network reads are explicit so callers can own their runtime.
#[derive(Clone)]
pub struct SessionInterruptHandle {
    socket: Arc<Mutex<Option<std::net::TcpStream>>>,
    cancelled: Arc<AtomicBool>,
    udp: Arc<Mutex<Option<Arc<UdpTransport>>>>,
    cancel_signal: tokio::sync::watch::Sender<bool>,
}

impl SessionInterruptHandle {
    pub fn interrupt(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.cancel_signal.send_replace(true);
        if let Ok(guard) = self.socket.lock() {
            if let Some(ref socket) = *guard {
                let _ = ferryx_rd_net::shutdown_socket(socket);
            }
        }
        if let Ok(guard) = self.udp.lock() {
            if let Some(udp) = guard.as_ref() {
                udp.cancel.send_replace(true);
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Progress emitted at the actual connection boundaries, independently of UI runtimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectPhase {
    Resolving,
    Connecting,
    Authenticating,
    AwaitingHostApproval,
    StartingVideo,
    Ready,
    Failed { reason: String },
}

type ProgressObserver = Arc<dyn Fn(ConnectPhase) + Send + Sync>;

#[derive(Clone)]
pub struct ClientSession {
    progress: Arc<Mutex<Option<ProgressObserver>>>,
    clock: Arc<Mutex<crate::clock::ClockCorrelation>>,
    config: SessionConfig,
    store: PairingStore,
    state: Arc<Mutex<SessionStateInner>>,
    tcp: Arc<Mutex<Option<TlsPskStream<std::net::TcpStream>>>>,
    interrupt_socket: Arc<Mutex<Option<std::net::TcpStream>>>,
    cancelled: Arc<AtomicBool>,
    // Lock before state/tcp when admitting sends or changing runtime ownership.
    clipboard_send: Arc<Mutex<()>>,
    image_sequence: Arc<AtomicU32>,
    input_send: Arc<Mutex<ferryx_rd_proto::InputJournal>>,
    cancel_signal: tokio::sync::watch::Sender<bool>,
    tcp_runtime: Arc<Mutex<Option<Arc<tokio::sync::Notify>>>>,
    udp: Arc<Mutex<Option<Arc<UdpTransport>>>>,
    udp_send: Arc<Mutex<Option<DatagramCipher>>>,
    udp_receive: Arc<Mutex<Option<DatagramCipher>>>,
    last_input_ack: Arc<Mutex<Option<(u32, bool, u8)>>>,
    udp_registered: Arc<AtomicBool>,
    registration_attempts: Arc<AtomicU32>,
    relay_endpoint: Arc<Mutex<Option<(String, u16, u16)>>>,
    /// Caller-provided endpoint (for example a loopback tunnel bridge) that every
    /// pairing connect and automatic reconnect uses instead of `config.host`.
    pinned_endpoint: Arc<Mutex<Option<(String, u16, u16)>>>,
    relay_stop: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    relay_worker: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    trace: ReceiverTrace,
    #[cfg(test)]
    tcp_wait: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    #[cfg(test)]
    tcp_write_wait: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}

impl ClientSession {
    pub fn new(config: SessionConfig) -> Result<Self, SessionError> {
        let store = match config.pairing_store_path.clone() {
            Some(path) => PairingStore::new(path),
            None => PairingStore::open_default()?,
        };
        Ok(Self {
            progress: Arc::new(Mutex::new(None)),
            clock: Arc::new(Mutex::new(crate::clock::ClockCorrelation::default())),
            config,
            store,
            state: Arc::new(Mutex::new(SessionStateInner::default())),
            tcp: Arc::new(Mutex::new(None)),
            interrupt_socket: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
            clipboard_send: Arc::new(Mutex::new(())),
            image_sequence: Arc::new(AtomicU32::new(0)),
            input_send: Arc::new(Mutex::new(ferryx_rd_proto::InputJournal::default())),
            cancel_signal: tokio::sync::watch::channel(false).0,
            tcp_runtime: Arc::new(Mutex::new(None)),
            udp: Arc::new(Mutex::new(None)),
            udp_send: Arc::new(Mutex::new(None)),
            udp_receive: Arc::new(Mutex::new(None)),
            last_input_ack: Arc::new(Mutex::new(None)),
            udp_registered: Arc::new(AtomicBool::new(false)),
            registration_attempts: Arc::new(AtomicU32::new(0)),
            relay_endpoint: Arc::new(Mutex::new(None)),
            pinned_endpoint: Arc::new(Mutex::new(None)),
            relay_stop: Arc::new(Mutex::new(None)),
            relay_worker: Arc::new(Mutex::new(None)),
            trace: std::env::var_os("MAHO_RECEIVER_TRACE_PATH")
                .map(|path| ReceiverTrace::at_path(path.into()))
                .unwrap_or_default(),
            #[cfg(test)]
            tcp_wait: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            tcp_write_wait: Arc::new(Mutex::new(None)),
        })
    }

    /// Install before connecting. The observer runs outside session locks.
    pub fn set_progress_observer(&self, observer: impl Fn(ConnectPhase) + Send + Sync + 'static) {
        *self.progress.lock().expect("progress lock poisoned") = Some(Arc::new(observer));
    }

    fn emit_progress(&self, phase: ConnectPhase) {
        let observer = self
            .progress
            .lock()
            .expect("progress lock poisoned")
            .clone();
        if let Some(observer) = observer {
            observer(phase);
        }
    }

    /// A renderer calls this after the first successfully submitted video frame.
    pub fn frame_presented(&self, id: u32) -> Result<(), SessionError> {
        let now = std::time::Instant::now();
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        let client_us = now.duration_since(state.parity.epoch).as_micros() as u64;
        state.receiver.present(
            id,
            now,
            client_us,
            &*self.clock.lock().map_err(|_| SessionError::Poisoned)?,
        );
        Ok(())
    }

    pub fn video_ready(&self) {
        self.emit_progress(ConnectPhase::Ready);
    }

    pub fn state(&self) -> Result<SessionState, SessionError> {
        Ok(self.state.lock().map_err(|_| SessionError::Poisoned)?.state)
    }

    pub fn session_info(&self) -> Result<SessionInfo, SessionError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .parity
            .info
            .clone())
    }

    pub fn session_stats(&self) -> Result<SessionStats, SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        let receiver = state.receiver.snapshot(std::time::Instant::now());
        let mut stats = state.parity.stats.clone();
        stats.receive_bytes_per_second = receiver.udp_receive_bps.map(|bps| bps / 8.0);
        stats.packet_loss_ratio = receiver.packet_loss_ratio;
        Ok(stats)
    }

    pub fn set_decoder_backend(&self, name: impl Into<String>) -> Result<(), SessionError> {
        self.state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .parity
            .stats
            .decoder_backend = Some(name.into());
        Ok(())
    }

    fn negotiated(&self, capability: Capabilities) -> Result<bool, SessionError> {
        let state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        Ok(self.config.capabilities.contains(capability)
            && state
                .server
                .as_ref()
                .is_some_and(|s| s.capabilities.contains(capability)))
    }

    pub fn request_secure_attention(&self) -> Result<u32, SessionError> {
        if !self.negotiated(Capabilities::SECURE_ATTENTION)? {
            return Err(SessionError::Unsupported);
        }
        let id = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            if state.parity.sas_pending.is_some() {
                return Err(SessionError::NotReady);
            }
            state.parity.next_sas = state
                .parity
                .next_sas
                .checked_add(1)
                .ok_or(SessionError::Unsupported)?;
            let id = state.parity.next_sas;
            state.parity.sas_pending = Some(id);
            id
        };
        if let Err(error) = self.send_control(ControlMessage::SecureAttentionRequest(
            ferryx_rd_proto::SecureAttentionRequest { request_id: id },
        )) {
            self.state
                .lock()
                .map_err(|_| SessionError::Poisoned)?
                .parity
                .sas_pending = None;
            return Err(error);
        }
        Ok(id)
    }

    pub fn set_clipboard_enabled(&self, enabled: bool) -> Result<(), SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        state.parity.clipboard_enabled = enabled;
        if !enabled {
            state.parity.images.discard();
            state.parity.completed_image = None;
        }
        Ok(())
    }

    pub fn take_clipboard_image(&self) -> Result<Option<Vec<u8>>, SessionError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .parity
            .completed_image
            .take())
    }

    pub fn effective_permissions(
        &self,
    ) -> Result<Option<ferryx_rd_proto::GuestPermissions>, SessionError> {
        let state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        Ok(self.permissions_locked(&state))
    }

    /// Single clipboard gate for every client push path. Negotiated
    /// GuestPermissions are authoritative; otherwise a HostPolicy decides; a
    /// host that sends neither (pre-policy builds) allows clipboard.
    pub fn clipboard_allowed(&self) -> Result<bool, SessionError> {
        let state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        Ok(state.parity.clipboard_enabled
            && self
                .permissions_locked(&state)
                .map_or(true, |p| p.clipboard))
    }

    fn permissions_locked(
        &self,
        state: &SessionStateInner,
    ) -> Option<ferryx_rd_proto::GuestPermissions> {
        if self
            .config
            .capabilities
            .contains(Capabilities::GUEST_PERMISSIONS)
            && state
                .server
                .as_ref()
                .is_some_and(|s| s.capabilities.contains(Capabilities::GUEST_PERMISSIONS))
        {
            Some(
                state
                    .parity
                    .info
                    .permissions
                    .unwrap_or(ferryx_rd_proto::GuestPermissions {
                        keyboard: false,
                        mouse: false,
                        gamepad: false,
                        clipboard: false,
                        audio: false,
                    }),
            )
        } else {
            state
                .stream_state
                .policy
                .map(|p| ferryx_rd_proto::GuestPermissions {
                    keyboard: p.allow_input,
                    mouse: p.allow_input,
                    gamepad: true,
                    clipboard: p.allow_clipboard,
                    audio: true,
                })
        }
    }

    pub fn send_clipboard_image(&self, png: &[u8]) -> Result<(), SessionError> {
        if !self.negotiated(Capabilities::CLIPBOARD_IMAGE)? {
            return Err(SessionError::Unsupported);
        }
        if !self.clipboard_allowed()? {
            return Err(SessionError::PermissionDenied);
        }
        if png.len() > ferryx_rd_proto::MAX_CLIPBOARD_IMAGE_BYTES as usize
            || !png.starts_with(ferryx_rd_proto::PNG_SIGNATURE)
        {
            return Err(SessionError::InvalidClipboardImage);
        }
        let _guard = self
            .clipboard_send
            .lock()
            .map_err(|_| SessionError::Poisoned)?;
        let mut current = self.image_sequence.load(Ordering::SeqCst);
        let id = loop {
            let next = current.checked_add(1).ok_or(SessionError::Unsupported)?;
            match self.image_sequence.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break next,
                Err(observed) => current = observed,
            }
        };
        for (index, chunk) in png
            .chunks(ferryx_rd_proto::MAX_CLIPBOARD_IMAGE_CHUNK_BYTES)
            .enumerate()
        {
            if !self.clipboard_allowed()? {
                return Err(SessionError::PermissionDenied);
            }
            self.send_control(ControlMessage::ClipboardImageChunk(
                ferryx_rd_proto::ClipboardImageChunk {
                    transfer_id: id,
                    total_bytes: png.len() as u32,
                    offset: (index * ferryx_rd_proto::MAX_CLIPBOARD_IMAGE_CHUNK_BYTES) as u32,
                    png_bytes: chunk.to_vec(),
                },
            ))?;
        }
        Ok(())
    }

    fn send_rtt_probe(&self) -> Result<(), SessionError> {
        if !self.negotiated(Capabilities::RTT_PROBE)? {
            return Ok(());
        }
        let timestamp = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            let now = std::time::Instant::now();
            if state
                .parity
                .probe
                .is_some_and(|(_, sent)| now.duration_since(sent) < Duration::from_secs(10))
            {
                return Ok(());
            }
            let timestamp = now.duration_since(state.parity.epoch).as_micros() as u64;
            state.parity.probe = Some((timestamp, now));
            timestamp
        };
        if self.negotiated(Capabilities::CLOCK_CORRELATION)? {
            self.send_control(ControlMessage::ClockPing(ferryx_rd_proto::ClockProbe {
                client_send_us: timestamp,
                host_receive_us: 0,
                host_send_us: 0,
            }))
        } else {
            self.send_control(ControlMessage::RttPing(ferryx_rd_proto::RttProbe {
                timestamp_us: timestamp,
            }))
        }
    }

    pub fn receiver_trace(&self) -> ReceiverTrace {
        self.trace.clone()
    }

    /// Returns the most recently received input acknowledgement (sequence, success, error_code).
    pub fn last_input_ack(&self) -> Option<(u32, bool, u8)> {
        *self.last_input_ack.lock().ok()?
    }

    /// Configure before connecting; clones made later share this endpoint trace.
    pub fn set_receiver_trace(&mut self, trace: ReceiverTrace) {
        self.trace = trace;
    }

    /// Returns bounded receiver observations at the current client monotonic time.
    pub fn receiver_snapshot(&self) -> Result<crate::ReceiverSnapshot, SessionError> {
        self.receiver_snapshot_at(std::time::Instant::now())
    }

    /// Explicit-clock snapshot; use nondecreasing times on the receiving clock.
    /// Reads finalize expired packet gaps, but never infer trailing traffic.
    pub fn receiver_snapshot_at(
        &self,
        now: std::time::Instant,
    ) -> Result<crate::ReceiverSnapshot, SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        let mut snapshot = state.receiver.snapshot(now);
        snapshot.abr_target_bitrate = state
            .abr
            .as_ref()
            .map(crate::AbrController::current_bitrate);
        Ok(snapshot)
    }

    pub fn pair_with_pin(&self, pin: &str) -> Result<ReadySession, SessionError> {
        self.pair_with_pin_as(pin, ferryx_rd_proto::PairingTrust::Owner)
    }

    pub fn pair_with_pin_cancellable(
        &self,
        pin: &str,
        handle: &SessionInterruptHandle,
    ) -> Result<ReadySession, SessionError> {
        if !Arc::ptr_eq(&self.cancelled, &handle.cancelled) {
            return Err(SessionError::Unsupported);
        }
        self.pair_with_pin(pin)
    }

    pub fn connect_with_pairing_cancellable(
        &self,
        pairing: PairingRecord,
        handle: &SessionInterruptHandle,
    ) -> Result<ReadySession, SessionError> {
        if !Arc::ptr_eq(&self.cancelled, &handle.cancelled) {
            return Err(SessionError::Unsupported);
        }
        self.connect_with_pairing(pairing)
    }

    pub fn pair_with_pin_as(
        &self,
        pin: &str,
        preference: ferryx_rd_proto::PairingTrust,
    ) -> Result<ReadySession, SessionError> {
        self.pair_with_pin_via_relay(pin, preference, None)
    }

    /// Tries the direct TLS-PSK bootstrap first, then the advertised relay route.
    /// Routing metadata is public; the PIN only enters the end-to-end TLS PSK.
    pub fn pair_with_pin_via_relay(
        &self,
        pin: &str,
        preference: ferryx_rd_proto::PairingTrust,
        relay: Option<&ferryx_rd_proto::RelayConnectCode>,
    ) -> Result<ReadySession, SessionError> {
        let result = self.pair_with_pin_inner(pin, preference, relay);
        if let Err(error) = &result {
            self.emit_progress(ConnectPhase::Failed {
                reason: error.to_string(),
            });
        }
        result
    }

    fn pair_with_pin_inner(
        &self,
        pin: &str,
        preference: ferryx_rd_proto::PairingTrust,
        relay: Option<&ferryx_rd_proto::RelayConnectCode>,
    ) -> Result<ReadySession, SessionError> {
        self.state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .parity
            .info
            .ended = None;
        let psk = PskIdentity::bootstrap(pin)?;
        self.stop_relay_bridge();
        if let Err(direct_error) = self.connect_with_psk(psk.clone(), SessionState::AwaitingPairing)
        {
            if self.is_cancelled() {
                return Err(SessionError::Cancelled);
            }
            let endpoint =
                relay.and_then(|route| self.open_relay_route(&route.url, &route.host_id));
            let Some(endpoint) = endpoint else {
                return Err(direct_error);
            };
            *self
                .relay_endpoint
                .lock()
                .map_err(|_| SessionError::Poisoned)? = Some(endpoint);
            self.connect_with_psk(psk, SessionState::AwaitingPairing)?;
        }
        self.send_packet(
            PacketType::PairingRequest,
            &PairingRequest {
                name: self.config.client_name.clone(),
            }
            .encode()?,
        )?;
        self.emit_progress(ConnectPhase::AwaitingHostApproval);
        let (header, payload) = self.read_tcp_packet()?;
        let pairing = match header.packet_type {
            PacketType::PairingGrant => {
                let grant = PairingGrant::decode(&payload)?;
                PairingRecord {
                    id: grant.pairing_id,
                    name: grant.host_name,
                    key: grant.key.to_vec(),
                    added_at_unix_ms: current_unix_ms(),
                    last_endpoint: None,
                    endpoint_aliases: Vec::new(),
                    relay_url: relay.map(|route| route.url.clone()),
                    relay_host_id: relay.map(|route| route.host_id.clone()),
                    trust: ferryx_rd_proto::PairingTrust::Owner,
                }
            }
            PacketType::PairingReject => {
                return Err(SessionError::PairingRejected(
                    PairingReject::decode(&payload)?.reason,
                ));
            }
            actual => {
                return Err(SessionError::UnexpectedPacket {
                    expected: PacketType::PairingGrant,
                    actual,
                });
            }
        };
        self.begin_handshake_with_trust(pairing, Some(preference))
    }

    pub fn reconnect(&self, pairing_id: &str) -> Result<ReadySession, SessionError> {
        let pairing = self
            .store
            .load(pairing_id)?
            .ok_or_else(|| SessionError::PairingNotFound(pairing_id.to_owned()))?;
        self.connect_with_pairing(pairing)
    }

    /// Connects directly using an explicit pairing record without consulting the pairing store.
    pub fn connect_with_pairing(
        &self,
        pairing: PairingRecord,
    ) -> Result<ReadySession, SessionError> {
        let result = self.connect_with_pairing_inner(pairing);
        if let Err(error) = &result {
            self.emit_progress(ConnectPhase::Failed {
                reason: error.to_string(),
            });
        }
        result
    }

    /// Connects with `pairing` through a caller-provided TCP/UDP endpoint such as a
    /// loopback bridge into an authenticated tunnel. TLS-PSK and the UDP AEAD stay end to
    /// end; the endpoint only carries bytes. Automatic reconnects reuse the same endpoint.
    pub fn connect_with_pairing_via(
        &self,
        pairing: PairingRecord,
        host: impl Into<String>,
        tcp_port: u16,
        udp_port: u16,
    ) -> Result<ReadySession, SessionError> {
        *self
            .pinned_endpoint
            .lock()
            .map_err(|_| SessionError::Poisoned)? = Some((host.into(), tcp_port, udp_port));
        self.connect_with_pairing(pairing)
    }

    fn connect_with_pairing_inner(
        &self,
        pairing: PairingRecord,
    ) -> Result<ReadySession, SessionError> {
        if self.is_cancelled() {
            return Err(SessionError::Cancelled);
        }
        self.state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .parity
            .info
            .ended = None;
        let psk = PskIdentity::pairing(&pairing.id, &pairing.key)?;
        let pinned = self
            .pinned_endpoint
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .clone();
        *self
            .relay_endpoint
            .lock()
            .map_err(|_| SessionError::Poisoned)? = pinned;
        match self.connect_with_psk(psk.clone(), SessionState::AwaitingHandshakeAck) {
            Ok(()) => self.begin_handshake(pairing),
            Err(direct_error) => {
                if self.is_cancelled() {
                    return Err(SessionError::Cancelled);
                }
                if let Some(endpoint) = self.open_relay_fallback(&pairing) {
                    *self
                        .relay_endpoint
                        .lock()
                        .map_err(|_| SessionError::Poisoned)? = Some(endpoint);
                    self.connect_with_psk(psk, SessionState::AwaitingHandshakeAck)?;
                    self.begin_handshake(pairing)
                } else {
                    Err(direct_error)
                }
            }
        }
    }

    /// True when the active connection runs through the relay fallback bridge
    /// rather than a direct path to the host.
    pub fn is_relayed(&self) -> bool {
        self.relay_endpoint
            .lock()
            .map(|endpoint| endpoint.is_some())
            .unwrap_or(false)
    }

    fn stop_relay_bridge(&self) {
        if let Ok(mut guard) = self.relay_stop.lock() {
            if let Some(stop) = guard.take() {
                let _ = stop.send(());
            }
        }
        if let Ok(mut worker) = self.relay_worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
        if let Ok(mut endpoint) = self.relay_endpoint.lock() {
            *endpoint = None;
        }
    }

    fn open_relay_fallback(&self, pairing: &PairingRecord) -> Option<(String, u16, u16)> {
        let url = pairing.relay_url.as_deref()?;
        let host_id = pairing.relay_host_id.clone().unwrap_or_else(|| {
            pairing
                .name
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
                .take(128)
                .collect()
        });
        self.open_relay_route(url, &host_id)
    }

    fn open_relay_route(&self, url: &str, host_id: &str) -> Option<(String, u16, u16)> {
        #[cfg(not(feature = "maho-relay"))]
        {
            let _ = (url, host_id);
            None
        }
        #[cfg(feature = "maho-relay")]
        {
            self.open_maho_relay_route(url, host_id)
        }
    }

    #[cfg(feature = "maho-relay")]
    fn open_maho_relay_route(&self, url: &str, host_id: &str) -> Option<(String, u16, u16)> {
        if !ferryx_rd_proto::relay_host_id_ok(host_id) {
            return None;
        }
        let secret = crate::relay::load_auth_secret()?;
        self.stop_relay_bridge();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let mut cancel = self.cancel_signal.subscribe();
        *self.relay_stop.lock().ok()? = Some(stop_tx);
        let url_for_thread = url.to_owned();
        let secret_for_thread = secret.into_bytes();
        let host_for_thread = host_id.to_owned();
        let worker = std::thread::Builder::new()
            .name("ferryx-rd-client-relay".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let bridge = tokio::select! {
                        biased;
                        _ = &mut stop_rx => return,
                        _ = cancel.changed() => return,
                        result = crate::relay::start_client_bridge(
                        &url_for_thread,
                        &host_for_thread,
                        &secret_for_thread,
                    ) => result,
                    };
                    match bridge {
                        Ok(bridge) => {
                            tracing::info!(
                                local_tcp = %bridge.local_tcp,
                                local_udp = %bridge.local_udp,
                                host_id = %host_for_thread,
                                "relay fallback bridge established"
                            );
                            let _ = ready_tx.send(Ok((
                                bridge.local_tcp.ip().to_string(),
                                bridge.local_tcp.port(),
                                bridge.local_udp.port(),
                            )));
                            tokio::select! { _ = &mut stop_rx => {}, _ = cancel.changed() => {} }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "relay fallback bridge failed");
                            let _ = ready_tx.send(Err(error));
                        }
                    }
                });
            })
            .ok()?;
        *self.relay_worker.lock().ok()? = Some(worker);
        let result = ready_rx.recv().ok().and_then(Result::ok);
        if result.is_none() {
            self.stop_relay_bridge();
        }
        result
    }

    pub fn set_udp_read_timeout(&self, timeout: Option<Duration>) -> Result<(), SessionError> {
        let udp = self.udp.lock().map_err(|_| SessionError::Poisoned)?;
        if let Some(udp) = udp.as_ref() {
            *udp.timeout.lock().map_err(|_| SessionError::Poisoned)? = timeout;
        }
        Ok(())
    }

    pub fn send_input(&self, event: InputEvent) -> Result<(), SessionError> {
        self.send_inputs(&[event])
    }

    pub fn send_inputs(&self, events: &[InputEvent]) -> Result<(), SessionError> {
        for event in events {
            self.check_input_permission(event)?;
        }
        let mut journal = self.input_send.lock().map_err(|_| SessionError::Poisoned)?;
        let negotiated = self.negotiated(Capabilities::UDP_INPUT)?;
        let mut offset = 0;
        while offset < events.len() {
            if negotiated && ferryx_rd_proto::is_pointer_motion(&events[offset]) {
                if journal.pending.len() == ferryx_rd_proto::MAX_INPUT_SAMPLES {
                    self.send_ready_packet(
                        PacketType::InputBatch,
                        &journal.checkpoint().encode()?,
                    )?;
                }
                let count = events[offset..]
                    .iter()
                    .take_while(|e| ferryx_rd_proto::is_pointer_motion(e))
                    .count()
                    .min(ferryx_rd_proto::MAX_INPUT_SAMPLES - journal.pending.len());
                let batch = journal.append(&events[offset..offset + count]);
                self.record_input_send(batch.sequence)?;
                self.send_udp(PacketType::InputBatch, &batch.encode()?)?;
                offset += count;
            } else {
                let mut payload = events[offset].encode()?;
                if negotiated {
                    if self.negotiated(Capabilities::INPUT_ECHO)? {
                        let batch = journal.append(&[]);
                        self.record_input_send(batch.sequence)?;
                    }
                    payload.extend(journal.checkpoint().encode()?);
                }
                self.send_ready_packet(PacketType::InputEvent, &payload)?;
                offset += 1;
            }
        }
        Ok(())
    }

    fn record_input_send(&self, sequence: u64) -> Result<(), SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        let now = std::time::Instant::now();
        let client_us = now.duration_since(state.parity.epoch).as_micros() as u64;
        state.receiver.input_sent(sequence, client_us);
        Ok(())
    }

    fn check_input_permission(&self, event: &InputEvent) -> Result<(), SessionError> {
        let state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        if let Some(p) = self.permissions_locked(&state) {
            use ferryx_rd_proto::InputEventType::*;
            let allowed = match event.event_type {
                Reset => true,
                KeyDown | KeyUp | FlagsChanged | UnicodeChar => p.keyboard,
                GamepadAxis | GamepadButtonDown | GamepadButtonUp => p.gamepad,
                _ => p.mouse,
            };
            if !allowed {
                return Err(SessionError::PermissionDenied);
            }
        }
        Ok(())
    }

    pub fn send_control(&self, control: ControlMessage) -> Result<(), SessionError> {
        if let Some(capability) = control.message_type().required_capability() {
            if !self.negotiated(capability)? {
                return Err(SessionError::Unsupported);
            }
        }
        if matches!(
            control,
            ControlMessage::ClipboardSyncUpdate(_) | ControlMessage::ClipboardImageChunk(_)
        ) && !self.clipboard_allowed()?
        {
            return Err(SessionError::PermissionDenied);
        }
        self.send_ready_packet(PacketType::Control, &control.encode()?)
    }

    /// Resends the authenticated UDP registration ping with a fresh AEAD nonce.
    pub fn send_udp_registration(&self) -> Result<(), SessionError> {
        let udp = {
            let socket_guard = self.udp.lock().map_err(|_| SessionError::Poisoned)?;
            socket_guard
                .as_ref()
                .cloned()
                .ok_or(SessionError::NotReady)?
        };
        let mut cipher_guard = self.udp_send.lock().map_err(|_| SessionError::Poisoned)?;
        let cipher = cipher_guard.as_mut().ok_or(SessionError::NotReady)?;
        let reg_header = PacketHeader::new(PacketType::Ping, 0, current_unix_ms() as u32, 0);
        let reg_packet = cipher.seal_datagram(&reg_header, &[])?;
        udp.socket.send(&reg_packet)?;
        Ok(())
    }

    pub fn send_udp(&self, packet_type: PacketType, payload: &[u8]) -> Result<(), SessionError> {
        let (sequence, udp) = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            if state.state != SessionState::Ready {
                return Err(SessionError::NotReady);
            }
            state.next_udp_sequence = state.next_udp_sequence.wrapping_add(1);
            let udp_guard = self.udp.lock().map_err(|_| SessionError::Poisoned)?;
            let udp = udp_guard.as_ref().cloned().ok_or(SessionError::NotReady)?;
            (state.next_udp_sequence, udp)
        };
        let header = PacketHeader::new(packet_type, sequence, current_unix_ms() as u32, 0);
        let mut cipher_guard = self.udp_send.lock().map_err(|_| SessionError::Poisoned)?;
        let datagram = cipher_guard
            .as_mut()
            .ok_or(SessionError::NotReady)?
            .seal_datagram(&header, payload)?;
        drop(cipher_guard);
        udp.socket.send(&datagram)?;
        Ok(())
    }

    pub fn send_bitrate_adjust(&self, target_bitrate: i32) -> Result<(), SessionError> {
        self.send_control(ControlMessage::BitrateAdjust(BitrateAdjust {
            target_bitrate,
        }))
    }

    pub fn evaluate_abr(
        &self,
        abr: &mut crate::AbrController,
        now: std::time::Instant,
    ) -> Result<Option<i32>, SessionError> {
        let (loss_ratio, delay_gradient, receive_bps) = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            (
                state.frames.congestion_loss_ratio(now),
                state.frames.delay_gradient_ms_per_s(now),
                state.receiver.abr_receive_bps(now),
            )
        };
        let target = abr.evaluate_signals(loss_ratio, delay_gradient, receive_bps, now);
        if let Some(target) = target {
            self.send_bitrate_adjust(target)?;
        }
        Ok(target)
    }

    /// Set user-selected bounds. Confirmed stream options seed the controller;
    /// authenticated receive ingress drives it without a frontend timer.
    pub fn configure_abr(&self, min_bitrate: i32, max_bitrate: i32) -> Result<(), SessionError> {
        if min_bitrate <= 0 || max_bitrate < min_bitrate {
            return Err(SessionError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid adaptive bitrate bounds",
            )));
        }
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        state.abr_bounds = Some((min_bitrate, max_bitrate));
        if let Some(current) = state
            .abr
            .as_ref()
            .map(crate::AbrController::current_bitrate)
        {
            state.confirm_abr(current as u32);
        }
        Ok(())
    }

    /// Also usable by an idle receive loop. No traffic means no upward probe.
    pub fn tick_abr(&self, now: std::time::Instant) -> Result<Option<i32>, SessionError> {
        let target = {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            if state.abr_tick.is_some_and(|last| {
                now.saturating_duration_since(last) < Duration::from_millis(200)
            }) {
                return Ok(None);
            }
            state.abr_tick = Some(now);
            let receive_bps = state.receiver.abr_receive_bps(now);
            let loss = state.frames.congestion_loss_ratio(now);
            let delay_gradient = state.frames.delay_gradient_ms_per_s(now);
            if receive_bps.is_none() && loss <= 0.05 && delay_gradient <= 10.0 {
                return Ok(None);
            }
            let Some(controller) = state.abr.as_mut() else {
                return Ok(None);
            };
            controller.evaluate_signals(loss, delay_gradient, receive_bps, now)
        };
        if let Some(target) = target {
            self.send_bitrate_adjust(target)?;
        }
        Ok(target)
    }

    pub fn send_clipboard_text(&self, text: String) -> Result<(), SessionError> {
        let _guard = self
            .clipboard_send
            .lock()
            .map_err(|_| SessionError::Poisoned)?;
        self.send_control(ControlMessage::ClipboardSyncUpdate(ClipboardSyncUpdate {
            request_id: 0,
            direction: ClipboardSyncDirection::ClientToHost,
            origin: ClipboardSyncOrigin::LocalPasteboard,
            text,
        }))
    }

    pub fn request_stream_config(
        &self,
        request_id: u32,
        desired: ferryx_rd_proto::StreamConfiguration,
    ) -> Result<(), SessionError> {
        self.send_control(ControlMessage::StreamConfigRequest(
            ferryx_rd_proto::StreamConfigurationRequest {
                request_id,
                desired,
            },
        ))
    }

    pub fn request_stream_options(
        &self,
        options: StreamOptionsRequest,
    ) -> Result<(), SessionError> {
        self.send_control(ControlMessage::StreamOptionsRequest(options))
    }

    /// Cached host cursor image announced over the reliable control stream.
    pub fn cursor_shape(
        &self,
        shape_id: u32,
    ) -> Result<Option<ferryx_rd_proto::CursorShape>, SessionError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .cursor_shapes
            .get(shape_id)
            .cloned())
    }

    pub fn stream_state(&self) -> Result<StreamState, SessionError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .stream_state
            .clone())
    }

    pub fn apply_remote_clipboard<C: PlatformClipboard>(
        &self,
        clipboard: &C,
        text: &str,
    ) -> Result<u64, SessionError> {
        clipboard
            .set_text(text)
            .map_err(|error| SessionError::Io(io::Error::other(error.to_string())))
    }

    pub fn receive_tcp_event(&self) -> Result<SessionEvent, SessionError> {
        let (header, payload) = self.read_tcp_packet()?;
        self.handle_packet(header, payload, None)
    }

    /// Starts the sole TCP readiness owner. Sends admit ordered frames to a
    /// bounded queue; saturation is reported as WouldBlock, never dropped.
    /// Stopping closes TCP and cancels all pending frames. Reconnect only after
    /// this runtime has stopped; a partial encrypted connection is not reusable.
    pub fn spawn_tcp_runtime(&self) -> Result<SessionRuntime, SessionError> {
        let mut generation = self.spawn_tcp_generation()?;
        let session = self.clone();
        let events = RuntimeEvents::default();
        let output = events.clone();
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let reconnecting = Arc::new(AtomicBool::new(false));
        let worker_reconnecting = reconnecting.clone();
        let worker = thread::spawn(move || {
            let mut next_probe = std::time::Instant::now();
            loop {
                if stopped.try_recv().is_ok()
                    || (session.is_cancelled()
                        && session
                            .session_info()
                            .map_or(true, |info| info.ended.is_none()))
                {
                    break;
                }
                if std::time::Instant::now() >= next_probe {
                    let _ = session.send_rtt_probe();
                    next_probe = std::time::Instant::now() + HEARTBEAT_INTERVAL;
                }
                match generation.events().recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(event)) => {
                        let terminal = matches!(
                            event,
                            SessionEvent::StreamConfig(ControlMessage::SessionEnded(_))
                        );
                        output.push(Ok(event));
                        if terminal {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    result => {
                        let error = match result {
                            Ok(Err(error)) => error,
                            _ => SessionError::TcpRuntimeStopped,
                        };
                        let _ = generation.stop();
                        let pairing = session.state.lock().ok().and_then(|state| {
                            if state.parity.info.ended.is_some() {
                                None
                            } else {
                                state.pairing.clone()
                            }
                        });
                        if session.is_cancelled()
                            && session
                                .session_info()
                                .map_or(true, |info| info.ended.is_none())
                        {
                            break;
                        }
                        if session.is_cancelled()
                            || !should_reconnect(&error)
                            || pairing
                                .as_ref()
                                .map_or(true, |p| p.trust == ferryx_rd_proto::PairingTrust::GuestOnce)
                        {
                            output.push(Err(error));
                            break;
                        }
                        let pairing = pairing.expect("eligible stored pairing");
                        worker_reconnecting.store(true, Ordering::SeqCst);
                        let deadline = std::time::Instant::now() + Duration::from_secs(60);
                        let mut attempt = 0;
                        let mut reason = error.to_string();
                        let recovered = loop {
                            attempt += 1;
                            let delay = parity::retry_delay(attempt);
                            if std::time::Instant::now() + delay >= deadline {
                                break false;
                            }
                            if let Ok(mut state) = session.state.lock() {
                                state.parity.info.reconnect = ReconnectState::Reconnecting {
                                    attempt,
                                    next_retry_ms: delay.as_millis() as u64,
                                };
                                state.parity.images.discard();
                                state.parity.probe = None;
                            }
                            let runtime = match tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                            {
                                Ok(runtime) => runtime,
                                Err(error) => {
                                    reason = error.to_string();
                                    break false;
                                }
                            };
                            let mut cancel = session.cancel_signal.subscribe();
                            let cancelled = runtime.block_on(async { tokio::select! { _ = &mut stopped => true, _ = cancel.changed() => true, _ = tokio::time::sleep(delay) => false } });
                            if cancelled || session.is_cancelled() {
                                break false;
                            }
                            let mut connector = session.clone();
                            let remaining =
                                deadline.saturating_duration_since(std::time::Instant::now());
                            connector.config.connect_timeout =
                                remaining.min(Duration::from_secs(5));
                            connector.config.handshake_ack_timeout =
                                remaining.min(Duration::from_secs(5));
                            match connector.connect_with_pairing(pairing.clone()) {
                                Ok(_) => match session.spawn_tcp_generation() {
                                    Ok(replacement) => {
                                        generation = replacement;
                                        break true;
                                    }
                                    Err(error) => {
                                        reason = error.to_string();
                                        break false;
                                    }
                                },
                                Err(error) => {
                                    reason = error.to_string();
                                    if !should_reconnect(&error) {
                                        break false;
                                    }
                                }
                            }
                        };
                        worker_reconnecting.store(false, Ordering::SeqCst);
                        if let Ok(mut state) = session.state.lock() {
                            state.parity.info.reconnect = if recovered {
                                ReconnectState::Reconnected
                            } else {
                                ReconnectState::GaveUp {
                                    reason: reason.clone(),
                                }
                            };
                        }
                        if !recovered {
                            output.push(Err(error));
                            break;
                        }
                        next_probe = std::time::Instant::now();
                    }
                }
            }
            let _ = generation.stop();
            output.close();
        });
        Ok(SessionRuntime {
            events,
            stop: Some(stop),
            worker: Some(worker),
            interrupt: Some((self.interrupt_handle(), reconnecting)),
        })
    }

    fn spawn_tcp_generation(&self) -> Result<SessionRuntime, SessionError> {
        if self.state()? != SessionState::Ready {
            return Err(SessionError::NotReady);
        }
        let readiness_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?;
        let mut owner = self
            .tcp_runtime
            .lock()
            .map_err(|_| SessionError::Poisoned)?;
        if owner.is_some() {
            return Err(SessionError::TcpRuntimeAlreadyRunning);
        }
        let readiness = {
            let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
            let socket = tcp
                .as_mut()
                .ok_or(SessionError::NotReady)?
                .ssl_stream()
                .get_ref();
            let observer = socket.try_clone()?;
            socket.set_nonblocking(true)?;
            let registered = {
                let _entered = readiness_runtime.enter();
                tokio::net::TcpStream::from_std(observer)
            };
            match registered {
                Ok(registered) => registered,
                Err(error) => {
                    socket.set_nonblocking(false)?;
                    return Err(error.into());
                }
            }
        };
        let wake = Arc::new(tokio::sync::Notify::new());
        *owner = Some(wake.clone());
        drop(owner);
        let session = self.clone();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let event_rx = RuntimeEvents::default();
        let event_tx = event_rx.clone();
        let worker = thread::spawn(move || {
            readiness_runtime.block_on(async { loop {
            if stop_rx.try_recv().is_ok() { break; }
            let mut interest = tokio::io::Interest::READABLE;
            let mut progressed = false;
            let result = {
                let mut tcp = session.tcp.lock().map_err(|_| SessionError::Poisoned);
                match tcp.as_mut() {
                    Ok(tcp) => match tcp.as_mut() {
                        Some(stream) => {
                            let written = if stream.has_pending_frames() {
                                match stream.write_frame_step() {
                                    Ok(()) => { progressed = true; Ok(()) }
                                    Err(TlsPskError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                                        #[cfg(test)]
                                        if let Some(wait) = session.tcp_write_wait.lock().unwrap().take() { wait.send(()).unwrap(); }
                                        if !stream.write_needs_read() { interest |= tokio::io::Interest::WRITABLE; }
                                        Ok(())
                                    }
                                    Err(error) => Err(SessionError::Tls(error)),
                                }
                            } else { Ok(()) };
                            // Resume SSL_write WANT_READ with SSL_write, not SSL_read.
                            if stream.has_pending_frames() && stream.write_needs_read() {
                                written.and(Err(SessionError::Tls(TlsPskError::Io(io::ErrorKind::WouldBlock.into()))))
                            } else {
                                written.and_then(|()| {
                                    let result = stream.read_frame_step().map_err(SessionError::from);
                                    if stream.read_needs_write() { interest |= tokio::io::Interest::WRITABLE; }
                                    result
                                })
                            }
                        }
                        None => Err(SessionError::NotReady),
                    },
                    Err(_) => Err(SessionError::Poisoned),
                }
            };
            let event = match result {
                Ok(Some(frame)) => {
                    if frame.len() < PacketHeader::SIZE {
                        Err(SessionError::Io(io::Error::from(io::ErrorKind::InvalidData)))
                    } else {
                        PacketHeader::decode(&frame[..PacketHeader::SIZE]).map_err(SessionError::from)
                            .and_then(|header| session.handle_packet(header, frame[PacketHeader::SIZE..].to_vec(), None))
                    }
                }
                Ok(None) => continue,
                Err(SessionError::Tls(TlsPskError::Io(error))) if error.kind() == io::ErrorKind::WouldBlock => {
                    if progressed { continue; }
                    #[cfg(test)]
                    if let Some(wait) = session.tcp_wait.lock().unwrap().take() { wait.send(()).unwrap(); }
                    tokio::select! {
                        biased;
                        _ = &mut stop_rx => break,
                        _ = wake.notified() => {},
                        ready = readiness.ready(interest) => {
                            if let Err(error) = ready { event_tx.push(Err(SessionError::Io(error))); break; }
                            let _: io::Result<()> = readiness.try_io(interest, || Err(io::ErrorKind::WouldBlock.into()));
                        }
                    }
                    continue;
                }
                Err(error) => Err(error),
            };
            if stop_rx.try_recv().is_ok() { break; }
            match event {
                Ok(SessionEvent::Ignored) => {}
                Ok(event) => {
                    event_tx.push(Ok(event));
                }
                Err(error) => {
                    event_tx.push(Err(error));
                    break;
                }
            }
        }});
            // Cancellation is terminal for this TCP generation. Drop the queue
            // and socket together, never reset a partial stream to blocking mode.
            let cleanup = (|| -> Result<(), SessionError> {
                let mut owner = session
                    .tcp_runtime
                    .lock()
                    .map_err(|_| SessionError::Poisoned)?;
                let mut tcp = session.tcp.lock().map_err(|_| SessionError::Poisoned)?;
                let result = tcp
                    .take()
                    .map(|tcp| ferryx_rd_net::shutdown_socket(tcp.ssl_stream().get_ref()));
                *owner = None;
                if let Ok(mut state) = session.state.lock() {
                    state.state = SessionState::Disconnected;
                }
                match result {
                    Some(Err(error)) if error.kind() != io::ErrorKind::NotConnected => {
                        Err(error.into())
                    }
                    _ => Ok(()),
                }
            })();
            if let Err(error) = cleanup {
                event_tx.push(Err(error));
            }
            event_tx.close();
        });
        Ok(SessionRuntime {
            events: event_rx,
            stop: Some(stop_tx),
            worker: Some(worker),
            interrupt: None,
        })
    }

    pub fn receive_udp_event(&self) -> Result<SessionEvent, SessionError> {
        self.receive_udp_event_clock(None)
    }

    /// Real socket/authentication ingress with an explicit receive clock for replay tests.
    pub fn receive_udp_event_at(
        &self,
        at: std::time::Instant,
    ) -> Result<SessionEvent, SessionError> {
        self.receive_udp_event_clock(Some(at))
    }

    fn receive_udp_event_clock(
        &self,
        at: Option<std::time::Instant>,
    ) -> Result<SessionEvent, SessionError> {
        let udp = {
            let socket_guard = self.udp.lock().map_err(|_| SessionError::Poisoned)?;
            socket_guard
                .as_ref()
                .cloned()
                .ok_or(SessionError::NotReady)?
        };
        let (datagram, count) = match udp.receive() {
            Ok(received) => received,
            Err(error) => {
                let mut record = ReceiverTraceRecord::new(ReceiverTraceEvent::ReceiveError);
                record.result = match &error {
                    SessionError::Io(error) => error.raw_os_error().unwrap_or(-1),
                    _ => -1,
                };
                self.trace.record(std::time::Instant::now(), record);
                if let SessionError::Io(ref io_err) = error {
                    if matches!(
                        io_err.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) && !self.udp_registered.load(Ordering::Relaxed)
                    {
                        let attempts = self.registration_attempts.fetch_add(1, Ordering::Relaxed);
                        if attempts < 10 {
                            let _ = self.send_udp_registration();
                        }
                    }
                }
                if matches!(&error, SessionError::Io(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock))
                {
                    self.send_recovery_feedback(at.unwrap_or_else(std::time::Instant::now))?;
                }
                return Err(error);
            }
        };
        let received_at = at.unwrap_or_else(|| datagram.received_at());
        let mut record = ReceiverTraceRecord::new(ReceiverTraceEvent::Receive);
        record.bytes = count as u64;
        if let Some(bytes) = datagram[..count].get(..PacketHeader::SIZE) {
            if let Ok(header) = PacketHeader::decode(bytes) {
                record.sequence = Some(header.sequence);
                record.kind = Some(header.packet_type as u8);
            }
        }
        self.trace.record(received_at, record);
        let mut cipher_guard = self
            .udp_receive
            .lock()
            .map_err(|_| SessionError::Poisoned)?;
        let opened = cipher_guard
            .as_mut()
            .ok_or(SessionError::NotReady)?
            .open_datagram(&datagram[..count]);
        let (header, payload) = match opened {
            Ok(packet) => packet,
            Err(error) => {
                record.event = ReceiverTraceEvent::AuthRejected;
                record.result = match error {
                    DatagramError::Authentication => 1,
                    DatagramError::Replay => 2,
                    DatagramError::Truncated => 3,
                    DatagramError::WrongDirection => 4,
                    DatagramError::InvalidHeader(_) => 5,
                    DatagramError::InvalidMasterKey => 6,
                    DatagramError::InvalidSessionSalt => 7,
                    DatagramError::CounterExhausted => 8,
                };
                self.trace.record(received_at, record);
                return Err(error.into());
            }
        };
        record.event = ReceiverTraceEvent::Authenticated;
        self.udp_registered.store(true, Ordering::Relaxed);
        record.frame = if self.trace.enabled() {
            match header.packet_type {
                PacketType::FrameHeader => FrameHeader::decode(&payload).ok().map(|h| h.frame_id),
                PacketType::FrameChunk => payload
                    .get(..4)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_le_bytes),
                PacketType::Ping => {
                    ferryx_rd_proto::TimestampStats::decode(&payload).map(|s| s.frame_id)
                }
                _ => None,
            }
        } else {
            None
        };
        self.trace.record(received_at, record);
        // Keep this cipher generation pinned until the authenticated observation
        // is consumed; a new handshake cannot install its cipher in between.
        let event = self.handle_packet(header, payload, Some((received_at, count)))?;
        drop(cipher_guard);
        drop(datagram);
        self.send_recovery_feedback(received_at)?;
        self.tick_abr(received_at)?;
        Ok(event)
    }

    pub fn interrupt_handle(&self) -> SessionInterruptHandle {
        SessionInterruptHandle {
            socket: self.interrupt_socket.clone(),
            cancelled: self.cancelled.clone(),
            udp: self.udp.clone(),
            cancel_signal: self.cancel_signal.clone(),
        }
    }

    pub fn interrupt(&self) {
        self.interrupt_handle().interrupt();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn disconnect(&self) -> Result<(), SessionError> {
        self.stop_relay_bridge();
        self.interrupt();
        let trace_result = self
            .receiver_snapshot()
            .and_then(|snapshot| self.trace.write(&snapshot).map_err(SessionError::Io));
        {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.state = SessionState::Disconnected;
            state.frames.clear();
            state.audio.clear();
            state.audio_playback = crate::audio_playback::AudioPlayback::default();
            state.receiver = crate::receiver_stats::ReceiverStats::default();
            state.abr = None;
            state.abr_tick = None;
            state.recovery_feedback_at = None;
            state.parity.images.discard();
            state.parity.completed_image = None;
            state.parity.probe = None;
            if state
                .pairing
                .as_ref()
                .is_some_and(|pairing| pairing.trust == ferryx_rd_proto::PairingTrust::GuestOnce)
            {
                state.pairing = None;
            }
        }
        if let Some(udp) = self.udp.lock().map_err(|_| SessionError::Poisoned)?.take() {
            udp.cancel.send_replace(true);
        }
        if let Some(tcp) = self.tcp.lock().map_err(|_| SessionError::Poisoned)?.take() {
            match ferryx_rd_net::shutdown_socket(tcp.ssl_stream().get_ref()) {
                Ok(()) => {}
                // The peer may have already closed; cancellation is already satisfied.
                Err(error) if error.kind() == io::ErrorKind::NotConnected => {}
                Err(error) => return Err(SessionError::Io(error)),
            }
        }
        *self.udp_send.lock().map_err(|_| SessionError::Poisoned)? = None;
        *self
            .udp_receive
            .lock()
            .map_err(|_| SessionError::Poisoned)? = None;
        trace_result
    }

    #[cfg(test)]
    fn udp_recv_buffer_ptr(&self) -> Option<usize> {
        let udp_guard = self.udp.lock().ok()?;
        let ptr = *udp_guard.as_ref()?.last_buffer_ptr.lock().ok()?;
        ptr
    }

    #[cfg(test)]
    fn udp_recv_alloc_count(&self) -> Option<usize> {
        let udp_guard = self.udp.lock().ok()?;
        let count = udp_guard
            .as_ref()?
            .alloc_count
            .load(std::sync::atomic::Ordering::SeqCst);
        Some(count)
    }

    #[cfg(test)]
    fn cancel_udp_receive_for_test(&self) {
        if let Ok(guard) = self.udp.lock() {
            if let Some(udp) = guard.as_ref() {
                udp.cancel.send_replace(true);
            }
        }
    }

    #[cfg(test)]
    fn reset_udp_cancel_for_test(&self) {
        if let Ok(guard) = self.udp.lock() {
            if let Some(udp) = guard.as_ref() {
                udp.cancel.send_replace(false);
            }
        }
    }

    #[cfg(test)]
    fn register_udp_entered_hook_for_test(&self, sender: mpsc::Sender<()>) {
        if let Ok(guard) = self.udp.lock() {
            if let Some(udp) = guard.as_ref() {
                *udp.entered.lock().unwrap() = Some(sender);
            }
        }
    }

    fn connect_with_psk(
        &self,
        psk: PskIdentity,
        next_state: SessionState,
    ) -> Result<(), SessionError> {
        let owner = self
            .tcp_runtime
            .lock()
            .map_err(|_| SessionError::Poisoned)?;
        if owner.is_some() {
            return Err(SessionError::TcpRuntimeAlreadyRunning);
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(SessionError::Cancelled);
        }
        {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.state = SessionState::Connecting;
            state.frames.clear();
            state.receiver = crate::receiver_stats::ReceiverStats::default();
            state.abr = None;
            state.abr_tick = None;
            state.parity.probe = None;
            state.parity.info.permissions = None;
            state.parity.info.host_status = None;
            state.parity.sas_pending = None;
            state.parity.images = parity::ImageAssembly::default();
            state.stream_state = StreamState::default();
            state.cursor = CursorState::default();
            state.cursor_shapes.clear();
        }
        self.emit_progress(ConnectPhase::Resolving);
        let address = {
            let override_endpoint = self
                .relay_endpoint
                .lock()
                .map_err(|_| SessionError::Poisoned)?;
            if let Some((host, tcp_port, _)) = override_endpoint.as_ref() {
                resolve_one((host.as_str(), *tcp_port))?
            } else {
                resolve_one((&*self.config.host, self.config.tcp_port))?
            }
        };
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(SessionError::Cancelled);
        }
        self.state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .receiver
            .set_trace(self.trace.clone());
        self.state
            .lock()
            .map_err(|_| SessionError::Poisoned)?
            .frames
            .set_receiver_trace(self.trace.clone());
        // This synchronous API is also called inside Tokio tasks. Keep the
        // bounded socket connect off the caller's runtime, and abandon its
        // result on cancellation (a late socket is dropped with the channel).
        self.emit_progress(ConnectPhase::Connecting);
        let timeout = self.config.connect_timeout;
        let deadline = std::time::Instant::now() + timeout;
        let (connected_tx, connected_rx) = mpsc::channel();
        thread::Builder::new()
            .name("ferryx-rd-client-connect".into())
            .spawn(move || {
                let _ = connected_tx.send(std::net::TcpStream::connect_timeout(&address, timeout));
            })?;
        let tcp = loop {
            if self.is_cancelled() {
                return Err(SessionError::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(SessionError::Io(io::ErrorKind::TimedOut.into()));
            }
            match connected_rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
                Ok(result) => break result?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(SessionError::TcpRuntimeStopped);
                }
            }
        };
        tcp.set_read_timeout(Some(self.config.connect_timeout))?;
        tcp.set_write_timeout(Some(self.config.connect_timeout))?;
        tcp.set_nodelay(true)?;

        let interrupt_clone = tcp.try_clone()?;
        *self
            .interrupt_socket
            .lock()
            .map_err(|_| SessionError::Poisoned)? = Some(interrupt_clone);

        if self.cancelled.load(Ordering::SeqCst) {
            let _ = tcp.shutdown(std::net::Shutdown::Both);
            return Err(SessionError::Cancelled);
        }

        self.emit_progress(ConnectPhase::Authenticating);
        let stream = match TlsPskClient::new(psk)?.connect_stream(tcp) {
            Ok(s) => s,
            Err(e) => {
                if self.cancelled.load(Ordering::SeqCst) {
                    return Err(SessionError::Cancelled);
                }
                return Err(SessionError::Tls(e));
            }
        };

        if self.cancelled.load(Ordering::SeqCst) {
            let _ = stream
                .ssl_stream()
                .get_ref()
                .shutdown(std::net::Shutdown::Both);
            return Err(SessionError::Cancelled);
        }

        *self.tcp.lock().map_err(|_| SessionError::Poisoned)? = Some(stream);
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        state.state = next_state;
        Ok(())
    }

    fn begin_handshake(&self, pairing: PairingRecord) -> Result<ReadySession, SessionError> {
        self.begin_handshake_with_trust(pairing, None)
    }

    fn begin_handshake_with_trust(
        &self,
        mut pairing: PairingRecord,
        preference: Option<ferryx_rd_proto::PairingTrust>,
    ) -> Result<ReadySession, SessionError> {
        let mut salt = [0_u8; 16];
        OsRng.fill_bytes(&mut salt);
        self.trace.begin_session(salt);
        let handshake = Handshake {
            name: self.config.client_name.clone(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::from_bits_retain(
                self.config.capabilities.bits() & SessionConfig::implemented_capabilities().bits(),
            ),
            pairing_id: pairing.id.clone(),
            session_salt: salt,
        };
        {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.pairing = Some(pairing.clone());
            state.session_salt = Some(salt);
            state.state = SessionState::AwaitingHandshakeAck;
            let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
            if let Some(tcp) = tcp.as_mut() {
                tcp.ssl_stream_mut()
                    .get_ref()
                    .set_read_timeout(Some(self.config.handshake_ack_timeout))?;
            }
        }
        self.send_packet(PacketType::Handshake, &handshake.encode()?)?;
        let (header, payload) = match self.read_tcp_packet() {
            Ok(packet) => packet,
            Err(SessionError::Tls(TlsPskError::Io(error)))
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(SessionError::HandshakeAckTimeout);
            }
            Err(error) => return Err(error),
        };
        if header.packet_type != PacketType::HandshakeAck {
            return Err(SessionError::UnexpectedPacket {
                expected: PacketType::HandshakeAck,
                actual: header.packet_type,
            });
        }
        let server = Handshake::decode(&payload)?;
        let trust_negotiated = self
            .config
            .capabilities
            .contains(Capabilities::PAIRING_TRUST)
            && server.capabilities.contains(Capabilities::PAIRING_TRUST);
        if trust_negotiated {
            if let Some(trust) = preference {
                self.send_packet(
                    PacketType::Control,
                    &ControlMessage::PairingTrustRequest(ferryx_rd_proto::PairingTrustRequest { trust })
                        .encode()?,
                )?;
            }
            let (header, payload) = self.read_tcp_packet()?;
            if header.packet_type != PacketType::Control {
                return Err(SessionError::UnexpectedPacket {
                    expected: PacketType::Control,
                    actual: header.packet_type,
                });
            }
            match ControlMessage::decode(&payload)? {
                ControlMessage::PairingTrustGrant(grant) => pairing.trust = grant.trust,
                ControlMessage::SessionEnded(ended)
                    if server.capabilities.contains(Capabilities::SESSION_ENDED)
                        && self
                            .config
                            .capabilities
                            .contains(Capabilities::SESSION_ENDED) =>
                {
                    self.state
                        .lock()
                        .map_err(|_| SessionError::Poisoned)?
                        .parity
                        .info
                        .ended = Some(ended.reason);
                    self.interrupt();
                    return Err(SessionError::PermissionDenied);
                }
                _ => return Err(SessionError::PermissionDenied),
            }
        } else if preference.is_some_and(|trust| trust != ferryx_rd_proto::PairingTrust::Owner) {
            self.interrupt();
            return Err(SessionError::Unsupported);
        }
        if pairing.trust != ferryx_rd_proto::PairingTrust::GuestOnce {
            if preference.is_some() {
                // Fresh bootstrap: persist only after host authorization.
                self.store.save(pairing.clone())?;
            } else if trust_negotiated {
                // Reconnect: record host-authoritative trust on the stored
                // record without overwriting fields persisted since connect.
                if let Some(mut stored) = self.store.load(&pairing.id)? {
                    if stored.trust != pairing.trust {
                        stored.trust = pairing.trust;
                        self.store.save(stored)?;
                    }
                }
            }
        }
        {
            let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
            state.pairing = Some(pairing.clone());
            state.parity.info.pairing_trust = Some(pairing.trust);
        }
        if !server
            .capabilities
            .contains(Capabilities::AUTHENTICATED_UDP_REGISTRATION)
        {
            return Err(SessionError::MissingAuthenticatedRegistration);
        }
        let key = pairing.key_array()?;
        let mut udp_send = DatagramCipher::derive(&key, &salt, Direction::ClientToHost)?;
        let udp_receive = DatagramCipher::derive(&key, &salt, Direction::HostToClient)?;
        let udp_address = {
            let override_endpoint = self
                .relay_endpoint
                .lock()
                .map_err(|_| SessionError::Poisoned)?;
            if let Some((host, _, udp_port)) = override_endpoint.as_ref() {
                resolve_one((host.as_str(), *udp_port))?
            } else {
                resolve_one((&*self.config.host, self.config.udp_port))?
            }
        };
        let udp = UdpSocket::bind(if udp_address.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        })?;
        if let Err(error) = ferryx_rd_net::udp_gcm::configure_media_qos(&udp, 34) {
            tracing::warn!(%error, "client media DSCP AF41 request failed; network priority is not guaranteed");
        }
        let receive_buffer =
            rustix::net::sockopt::set_socket_recv_buffer_size(&udp, 4 * 1024 * 1024);
        let mut record = ReceiverTraceRecord::new(ReceiverTraceEvent::SocketBuffer);
        record.count = 4 * 1024 * 1024;
        record.result = receive_buffer.err().map_or(0, |error| error.raw_os_error());
        match rustix::net::sockopt::socket_recv_buffer_size(&udp) {
            Ok(bytes) => record.bytes = bytes as u64,
            Err(error) => {
                record.sequence = Some(error.raw_os_error() as u32);
            }
        }
        self.trace.record(std::time::Instant::now(), record);
        if let Err(error) = rustix::net::sockopt::set_socket_send_buffer_size(&udp, 4 * 1024 * 1024)
        {
            tracing::warn!(%error, "unable to request UDP send buffer");
        }
        match rustix::net::sockopt::socket_send_buffer_size(&udp) {
            Ok(bytes) => {
                // Client sends input, not video keyframes. The host must perform
                // the same check against its largest encoded keyframe.
                tracing::debug!(bytes, "effective client UDP send buffer");
                if bytes < 4 * 1024 * 1024 {
                    tracing::warn!(bytes, requested = 4 * 1024 * 1024,
                        "UDP send buffer clamped; Linux: raise net.core.wmem_max; macOS: raise kern.ipc.maxsockbuf and net.inet.udp.maxdgram; pace bursts rather than relying on buffering");
                }
            }
            Err(error) => tracing::warn!(%error, "cannot read effective UDP send buffer"),
        }
        udp.connect(udp_address)?;
        udp.set_read_timeout(Some(
            if self.config.capabilities.contains(Capabilities::FEC_NACK)
                && server.capabilities.contains(Capabilities::FEC_NACK)
            {
                Duration::from_millis(4)
            } else {
                HEARTBEAT_INTERVAL * 3
            },
        ))?;
        self.udp_registered.store(false, Ordering::Relaxed);
        self.registration_attempts.store(0, Ordering::Relaxed);
        let reg_header = PacketHeader::new(PacketType::Ping, 0, current_unix_ms() as u32, 0);
        let reg_packet = udp_send.seal_datagram(&reg_header, &[])?;
        let _ = udp.send(&reg_packet);
        {
            let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
            if let Some(tcp) = tcp.as_mut() {
                tcp.ssl_stream_mut().get_ref().set_read_timeout(None)?;
            }
        }
        let ready = ReadySession {
            pairing: pairing.clone(),
            server: server.clone(),
            session_salt: salt,
        };
        *self.udp.lock().map_err(|_| SessionError::Poisoned)? =
            Some(Arc::new(UdpTransport::new(udp)?));
        *self.udp_send.lock().map_err(|_| SessionError::Poisoned)? = Some(udp_send);
        *self
            .udp_receive
            .lock()
            .map_err(|_| SessionError::Poisoned)? = Some(udp_receive);
        let host_supports_stream_options = self
            .config
            .capabilities
            .contains(Capabilities::STREAM_OPTIONS)
            && server.capabilities.contains(Capabilities::STREAM_OPTIONS);
        *self.input_send.lock().map_err(|_| SessionError::Poisoned)? =
            ferryx_rd_proto::InputJournal::default();
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        state.stream_state.host_supports_stream_options = host_supports_stream_options;
        state.server = Some(server);
        state.state = SessionState::Ready;
        drop(state);
        self.emit_progress(ConnectPhase::StartingVideo);
        Ok(ready)
    }

    fn send_ready_packet(
        &self,
        packet_type: PacketType,
        payload: &[u8],
    ) -> Result<(), SessionError> {
        if self.state()? != SessionState::Ready {
            return Err(SessionError::NotReady);
        }
        self.send_packet(packet_type, payload)
    }

    fn send_packet(&self, packet_type: PacketType, payload: &[u8]) -> Result<(), SessionError> {
        let owner = self
            .tcp_runtime
            .lock()
            .map_err(|_| SessionError::Poisoned)?;
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        let sequence = state.next_tcp_sequence.wrapping_add(1);
        let header = PacketHeader::new(packet_type, sequence, current_unix_ms() as u32, 0);
        let mut packet = header.encode()?;
        packet.extend_from_slice(payload);
        let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
        let stream = tcp.as_mut().ok_or(SessionError::NotReady)?;
        if let Some(wake) = owner.as_ref() {
            stream.queue_frame(&packet)?;
            // Preserve immediate delivery when writable (notably Disconnect
            // immediately followed by stop). This never waits: the socket stays
            // nonblocking and only retained, hard-bounded data is attempted.
            let result = stream.flush_pending_frames();
            wake.notify_one();
            match result {
                Ok(()) => {}
                Err(TlsPskError::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            stream.write_frame(&packet)?;
        }
        state.next_tcp_sequence = sequence;
        Ok(())
    }

    fn read_tcp_packet(&self) -> Result<(PacketHeader, Vec<u8>), SessionError> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(SessionError::Cancelled);
        }
        let read_result = {
            let mut tcp = self.tcp.lock().map_err(|_| SessionError::Poisoned)?;
            tcp.as_mut().ok_or(SessionError::NotReady)?.read_frame()
        };
        let frame = match read_result {
            Ok(f) => f,
            Err(e) => {
                if self.cancelled.load(Ordering::SeqCst) {
                    return Err(SessionError::Cancelled);
                }
                return Err(e.into());
            }
        };
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(SessionError::Cancelled);
        }
        if frame.len() < PacketHeader::SIZE {
            return Err(SessionError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                "TCP packet is shorter than the v3 header",
            )));
        }
        Ok((
            PacketHeader::decode(&frame[..PacketHeader::SIZE])?,
            frame[PacketHeader::SIZE..].to_vec(),
        ))
    }

    fn handle_packet(
        &self,
        header: PacketHeader,
        payload: Vec<u8>,
        udp_observation: Option<(std::time::Instant, usize)>,
    ) -> Result<SessionEvent, SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        if state.state != SessionState::Ready {
            return Err(SessionError::NotReady);
        }
        let from_udp = udp_observation.is_some();
        let received_at = if let Some((at, bytes)) = udp_observation {
            // Only receive_udp_event supplies this after successful authentication.
            // Count transport bytes even if the authenticated media payload is invalid.
            state.receiver.observe_datagram(header.sequence, bytes, at);
            at
        } else {
            std::time::Instant::now()
        };
        match header.packet_type {
            PacketType::FrameHeader | PacketType::FrameChunk if from_udp => {
                let frame = if header.packet_type == PacketType::FrameHeader {
                    state.frames.push_header(
                        FrameHeader::decode(&payload)?,
                        header.timestamp_ms,
                        received_at,
                    )?
                } else {
                    state
                        .frames
                        .push_chunk(FrameChunk::decode(&payload)?, received_at)?
                };
                if let Some(started_at) = state.frames.take_completed_started_at() {
                    // Completion time comes from the same receive clock that
                    // started the assembly: mixing a supplied logical start
                    // with a wall-clock end would invert or skew the interval
                    // under explicit-clock replay ingress.
                    state.receiver.record_assembly(started_at, received_at);
                }
                if let Some(frame) = &frame {
                    let client_us =
                        received_at.duration_since(state.parity.epoch).as_micros() as u64;
                    state
                        .receiver
                        .frame_arrived(frame.header.frame_id, client_us, received_at);
                }
                Ok(frame.map_or(SessionEvent::Ignored, SessionEvent::Frame))
            }
            PacketType::FrameParity
                if from_udp
                    && self.config.capabilities.contains(Capabilities::FEC_NACK)
                    && state
                        .server
                        .as_ref()
                        .is_some_and(|s| s.capabilities.contains(Capabilities::FEC_NACK)) =>
            {
                let frame = state
                    .frames
                    .push_parity(ferryx_rd_proto::FrameParity::decode(&payload)?, received_at)?;
                if let Some(started_at) = state.frames.take_completed_started_at() {
                    state.receiver.record_assembly(started_at, received_at);
                }
                if let Some(frame) = &frame {
                    let client_us =
                        received_at.duration_since(state.parity.epoch).as_micros() as u64;
                    state
                        .receiver
                        .frame_arrived(frame.header.frame_id, client_us, received_at);
                }
                Ok(frame.map_or(SessionEvent::Ignored, SessionEvent::Frame))
            }
            PacketType::AudioFrame
                if from_udp && self.permissions_locked(&state).is_some_and(|p| !p.audio) =>
            {
                Ok(SessionEvent::Ignored)
            }
            PacketType::AudioFrame if from_udp => {
                let opus = self.config.capabilities.contains(Capabilities::AUDIO_OPUS)
                    && state
                        .server
                        .as_ref()
                        .is_some_and(|s| s.capabilities.contains(Capabilities::AUDIO_OPUS));
                let source_time =
                    self.config
                        .capabilities
                        .contains(Capabilities::AUDIO_SOURCE_TIME)
                        && state.server.as_ref().is_some_and(|s| {
                            s.capabilities.contains(Capabilities::AUDIO_SOURCE_TIME)
                        });
                let fragment = AudioFragment::decode(&payload)?;
                let frame_id = fragment.header.frame_id;
                match state.audio.push(fragment) {
                    Some(bytes) => {
                        let (capture_us, data) = ferryx_rd_proto::split_audio_source_time(
                            &bytes,
                            if source_time {
                                Capabilities::AUDIO_SOURCE_TIME
                            } else {
                                Capabilities::empty()
                            },
                        )
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "truncated audio source time",
                            )
                        })?;
                        state.receiver.record_audio_source(capture_us);
                        Ok(state
                            .audio_playback
                            .decode_frame(frame_id, data.to_vec(), opus)?
                            .map_or(SessionEvent::Ignored, SessionEvent::Audio))
                    }
                    None => Ok(SessionEvent::Ignored),
                }
            }
            PacketType::CursorUpdate if from_udp => {
                let shaped = self
                    .config
                    .capabilities
                    .contains(Capabilities::CURSOR_SHAPE)
                    && state
                        .server
                        .as_ref()
                        .is_some_and(|s| s.capabilities.contains(Capabilities::CURSOR_SHAPE));
                let (update, shape_id) = CursorUpdate::decode_with_shape(&payload, shaped)?;
                state.cursor.update_with_shape(update, shape_id);
                Ok(SessionEvent::Cursor(state.cursor))
            }
            PacketType::Ping if from_udp => {
                if self.config.capabilities.contains(Capabilities::INPUT_ECHO)
                    && state
                        .server
                        .as_ref()
                        .is_some_and(|s| s.capabilities.contains(Capabilities::INPUT_ECHO))
                {
                    if let Some(echo) = ferryx_rd_proto::InputEcho::decode(&payload) {
                        state.receiver.input_echo(echo, received_at);
                    }
                }
                if let Some(stats) = ferryx_rd_proto::TimestampStats::decode(&payload) {
                    state.receiver.record_host(stats, received_at);
                    let client_us =
                        received_at.duration_since(state.parity.epoch).as_micros() as u64;
                    state.receiver.record_network(
                        stats,
                        received_at,
                        client_us,
                        &*self.clock.lock().map_err(|_| SessionError::Poisoned)?,
                    );
                }
                Ok(SessionEvent::Ping)
            }
            PacketType::Ping => {
                drop(state);
                self.send_control(ControlMessage::Pong)?;
                Ok(SessionEvent::Ping)
            }
            PacketType::Control => {
                let message = ControlMessage::decode(&payload)?;
                if let Some(capability) = message.message_type().required_capability() {
                    if !self.config.capabilities.contains(capability)
                        || !state
                            .server
                            .as_ref()
                            .is_some_and(|s| s.capabilities.contains(capability))
                    {
                        return Err(SessionError::Unsupported);
                    }
                }
                match message {
                    ControlMessage::HostStatus(status) => {
                        state.parity.info.host_status = Some(status.clone());
                        Ok(SessionEvent::StreamConfig(ControlMessage::HostStatus(
                            status,
                        )))
                    }
                    ControlMessage::GuestPermissions(permissions) => {
                        let removed = state.parity.info.permissions.is_some_and(|old| {
                            (old.keyboard && !permissions.keyboard)
                                || (old.mouse && !permissions.mouse)
                                || (old.gamepad && !permissions.gamepad)
                        });
                        state.parity.info.permissions = Some(permissions);
                        state.stream_state.policy = Some(HostPolicy {
                            allow_input: permissions.keyboard && permissions.mouse,
                            allow_clipboard: permissions.clipboard,
                        });
                        if !permissions.clipboard {
                            state.parity.images.discard();
                            state.parity.completed_image = None;
                        }
                        drop(state);
                        if removed {
                            self.send_input(InputEvent {
                                event_type: ferryx_rd_proto::InputEventType::Reset,
                                ..InputEvent::unicode_char(0, 0.0, 0.0)
                            })?;
                        }
                        Ok(SessionEvent::StreamConfig(
                            ControlMessage::GuestPermissions(permissions),
                        ))
                    }
                    ControlMessage::SessionEnded(ended) => {
                        state.parity.info.ended = Some(ended.reason);
                        state.parity.images.discard();
                        state.parity.completed_image = None;
                        state.frames.clear();
                        state.audio.clear();
                        state.audio_playback = crate::audio_playback::AudioPlayback::default();
                        drop(state);
                        let _ = self.send_input(InputEvent {
                            event_type: ferryx_rd_proto::InputEventType::Reset,
                            ..InputEvent::unicode_char(0, 0.0, 0.0)
                        });
                        self.interrupt();
                        self.state.lock().map_err(|_| SessionError::Poisoned)?.state =
                            SessionState::Disconnected;
                        Ok(SessionEvent::StreamConfig(ControlMessage::SessionEnded(
                            ended,
                        )))
                    }
                    ControlMessage::SecureAttentionResponse(response) => {
                        if state.parity.sas_pending == Some(response.request_id) {
                            state.parity.sas_pending = None;
                            state.parity.info.secure_attention_response = Some(response);
                            Ok(SessionEvent::StreamConfig(
                                ControlMessage::SecureAttentionResponse(response),
                            ))
                        } else {
                            Ok(SessionEvent::Ignored)
                        }
                    }
                    ControlMessage::ClockPong(probe) => {
                        let now = std::time::Instant::now();
                        if state
                            .parity
                            .probe
                            .is_some_and(|(sent, _)| sent == probe.client_send_us)
                        {
                            let received =
                                now.duration_since(state.parity.epoch).as_micros() as u64;
                            self.clock
                                .lock()
                                .map_err(|_| SessionError::Poisoned)?
                                .observe(probe, received);
                            state.parity.pong(probe.client_send_us, now);
                        }
                        Ok(SessionEvent::Ignored)
                    }
                    ControlMessage::RttPong(probe) => {
                        state
                            .parity
                            .pong(probe.timestamp_us, std::time::Instant::now());
                        Ok(SessionEvent::Ignored)
                    }
                    ControlMessage::ClipboardImageChunk(chunk) => {
                        if !state.parity.clipboard_enabled
                            || self
                                .permissions_locked(&state)
                                .is_some_and(|p| !p.clipboard)
                        {
                            return Err(SessionError::PermissionDenied);
                        }
                        if let Some(png) =
                            state.parity.images.push(chunk, std::time::Instant::now())?
                        {
                            state.parity.completed_image = Some(png);
                        }
                        Ok(SessionEvent::Ignored)
                    }
                    ControlMessage::CursorShape(shape) => {
                        // Images are reliable control data, never UDP media.
                        if from_udp {
                            return Err(SessionError::Unsupported);
                        }
                        // Capability was checked above; consumers read the
                        // image by id through `cursor_shape`.
                        state.cursor_shapes.insert(shape);
                        Ok(SessionEvent::Ignored)
                    }
                    ControlMessage::Ping => {
                        drop(state);
                        self.send_control(ControlMessage::Pong)?;
                        Ok(SessionEvent::Ping)
                    }
                    ControlMessage::ClipboardSyncUpdate(update) => {
                        state.parity.images.discard();
                        state.parity.completed_image = None;
                        if !state.parity.clipboard_enabled
                            || self
                                .permissions_locked(&state)
                                .is_some_and(|p| !p.clipboard)
                        {
                            return Ok(SessionEvent::Ignored);
                        }
                        Ok(SessionEvent::Clipboard(update.text))
                    }
                    ControlMessage::InputAck(ack) => {
                        if let Ok(mut lock) = self.last_input_ack.lock() {
                            *lock = Some((ack.sequence, ack.success, ack.error_code));
                        }
                        Ok(SessionEvent::InputAck {
                            sequence: ack.sequence,
                            success: ack.success,
                            error_code: ack.error_code,
                        })
                    }
                    ControlMessage::StreamOptionsActive(active) => {
                        state.confirm_abr(active.bitrate);
                        state.stream_state.active = Some(active);
                        Ok(SessionEvent::StreamConfig(
                            ControlMessage::StreamOptionsActive(active),
                        ))
                    }
                    ControlMessage::HostDisplayList(list) => {
                        state.stream_state.displays = list.displays.clone();
                        Ok(SessionEvent::StreamConfig(ControlMessage::HostDisplayList(
                            list,
                        )))
                    }
                    ControlMessage::HostPolicy(policy) => {
                        if !self
                            .config
                            .capabilities
                            .contains(Capabilities::GUEST_PERMISSIONS)
                            || !state.server.as_ref().is_some_and(|s| {
                                s.capabilities.contains(Capabilities::GUEST_PERMISSIONS)
                            })
                        {
                            state.stream_state.policy = Some(policy);
                        }
                        Ok(SessionEvent::StreamConfig(ControlMessage::HostPolicy(
                            policy,
                        )))
                    }
                    ControlMessage::StreamConfigReject(reject) => {
                        state.stream_state.last_reject = Some(reject.message.clone());
                        Ok(SessionEvent::StreamConfig(
                            ControlMessage::StreamConfigReject(reject),
                        ))
                    }
                    ControlMessage::StreamConfigResponse(response) => {
                        state.confirm_abr(response.active.bitrate);
                        Ok(SessionEvent::StreamConfig(
                            ControlMessage::StreamConfigResponse(response),
                        ))
                    }
                    msg @ ControlMessage::StreamConfigError(_) => {
                        Ok(SessionEvent::StreamConfig(msg))
                    }
                    _ => Ok(SessionEvent::Ignored),
                }
            }
            _ => Ok(SessionEvent::Ignored),
        }
    }
}

/// Bytes reserved per received datagram: the full UDP payload range, so a
/// large packet after a small one never truncates.
const UDP_RECV_SLOT: usize = 65_536;
/// Datagrams drained per receive syscall. 64-bit Linux batches with
/// `recvmmsg`; 32-bit Linux (whose musl `msghdr` layout differs) and other
/// platforms cost one `recv` per datagram.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
const UDP_RECV_BATCH: usize = 8;
#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
const UDP_RECV_BATCH: usize = 1;
/// Upper bound on one blocking read so cancellation is observed promptly.
const UDP_RECV_POLL_SLICE: Duration = Duration::from_millis(25);

/// Minimal `recvmmsg(2)` binding; rustix 1.x exposes `sendmmsg` only.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod udp_mmsg {
    use std::ffi::{c_int, c_uint, c_void};

    #[repr(C)]
    pub(super) struct IoVec {
        pub base: *mut c_void,
        pub len: usize,
    }

    /// Only compiled for 64-bit Linux. `msg_iovlen`/`msg_controllen` are
    /// `size_t` on glibc; musl declares `int` plus an endian-ordered 32-bit
    /// pad that places the value where a `usize` store of a small count lands.
    #[repr(C)]
    pub(super) struct MsgHdr {
        pub name: *mut c_void,
        pub name_len: u32,
        pub iov: *mut IoVec,
        pub iov_len: usize,
        pub control: *mut c_void,
        pub control_len: usize,
        pub flags: c_int,
    }

    #[repr(C)]
    pub(super) struct MMsgHdr {
        pub hdr: MsgHdr,
        pub len: c_uint,
    }

    /// Block for the first datagram only, then drain what is already queued.
    pub(super) const MSG_WAITFORONE: c_int = 0x10000;

    extern "C" {
        pub(super) fn recvmmsg(
            fd: c_int,
            msgs: *mut MMsgHdr,
            vlen: c_uint,
            flags: c_int,
            timeout: *mut c_void,
        ) -> c_int;
    }
}

struct UdpRecvState {
    /// `UDP_RECV_BATCH` slots of `UDP_RECV_SLOT` bytes, allocated once.
    slab: Vec<u8>,
    lens: [usize; UDP_RECV_BATCH],
    /// Next queued slot to hand out; slots `next..filled` are pending.
    next: usize,
    filled: usize,
    received_at: std::time::Instant,
    /// `SO_RCVTIMEO` currently on the socket, so it is only rewritten when
    /// the poll slice actually changes.
    applied_slice: Option<Duration>,
}

/// One received datagram, borrowed from the transport's receive slab until
/// dropped. Derefs to the slot; callers index `[..count]`.
struct UdpDatagram<'a> {
    state: std::sync::MutexGuard<'a, UdpRecvState>,
    slot: usize,
}

impl UdpDatagram<'_> {
    fn received_at(&self) -> std::time::Instant {
        self.state.received_at
    }
}

impl std::ops::Deref for UdpDatagram<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        let start = self.slot * UDP_RECV_SLOT;
        &self.state.slab[start..start + UDP_RECV_SLOT]
    }
}

struct UdpTransport {
    socket: UdpSocket,
    cancel: tokio::sync::watch::Sender<bool>,
    timeout: Mutex<Option<Duration>>,
    state: Mutex<UdpRecvState>,
    #[cfg(test)]
    alloc_count: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    recv_syscalls: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    timeout_syscalls: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    last_buffer_ptr: Mutex<Option<usize>>,
    #[cfg(test)]
    entered: Mutex<Option<mpsc::Sender<()>>>,
}

impl UdpTransport {
    fn new(socket: UdpSocket) -> Result<Self, SessionError> {
        let timeout = socket.read_timeout()?;
        let (cancel, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            socket,
            cancel,
            timeout: Mutex::new(timeout),
            state: Mutex::new(UdpRecvState {
                slab: Vec::new(),
                lens: [0; UDP_RECV_BATCH],
                next: 0,
                filled: 0,
                received_at: std::time::Instant::now(),
                applied_slice: timeout,
            }),
            #[cfg(test)]
            alloc_count: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            recv_syscalls: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            timeout_syscalls: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            last_buffer_ptr: Mutex::new(None),
            #[cfg(test)]
            entered: Mutex::new(None),
        })
    }

    /// Returns the next authenticated-pending datagram, one per call; batched
    /// datagrams are queued and handed out individually.
    fn receive(&self) -> Result<(UdpDatagram<'_>, usize), SessionError> {
        let mut state = self.state.lock().map_err(|_| SessionError::Poisoned)?;
        if state.slab.is_empty() {
            state.slab.resize(UDP_RECV_SLOT * UDP_RECV_BATCH, 0);
            #[cfg(test)]
            self.alloc_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        let timeout = *self.timeout.lock().map_err(|_| SessionError::Poisoned)?;
        let deadline = timeout.map(|timeout| std::time::Instant::now() + timeout);
        #[cfg(test)]
        {
            *self.last_buffer_ptr.lock().unwrap() = Some(state.slab.as_ptr() as usize);
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered
                    .send(())
                    .map_err(|_| SessionError::TcpRuntimeStopped)?;
            }
        }
        let mut first_read = true;
        // Bounded std socket reads preserve cancellation without owning or
        // entering a runtime on the synchronous caller's thread.
        loop {
            if *self.cancel.borrow() {
                return Err(SessionError::NotReady);
            }
            if state.next < state.filled {
                let slot = state.next;
                state.next += 1;
                let count = state.lens[slot];
                return Ok((UdpDatagram { state, slot }, count));
            }
            let slice = match deadline {
                Some(_) if first_read => timeout
                    .expect("deadline requires timeout")
                    .min(UDP_RECV_POLL_SLICE),
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(SessionError::Io(io::ErrorKind::TimedOut.into()));
                    }
                    // Whole milliseconds keep a short fixed timeout from
                    // yielding a fresh nanosecond slice (and setsockopt) per
                    // datagram; overshoot is below 1ms. Rounding up from
                    // nanoseconds keeps a sub-microsecond remainder at 1ms:
                    // a zero socket timeout is rejected by std.
                    let millis = remaining.as_nanos().div_ceil(1_000_000);
                    Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
                        .min(UDP_RECV_POLL_SLICE)
                }
                None => UDP_RECV_POLL_SLICE,
            };
            if state.applied_slice != Some(slice) {
                self.socket.set_read_timeout(Some(slice))?;
                state.applied_slice = Some(slice);
                #[cfg(test)]
                self.timeout_syscalls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            first_read = false;
            match self.fill(&mut state) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Blocks up to the applied `SO_RCVTIMEO` for at least one datagram and
    /// queues every datagram one syscall can deliver.
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    fn fill(&self, state: &mut UdpRecvState) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        #[cfg(test)]
        self.recv_syscalls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        state.next = 0;
        state.filled = 0;
        let base = state.slab.as_mut_ptr();
        let mut iov: [udp_mmsg::IoVec; UDP_RECV_BATCH] =
            std::array::from_fn(|slot| udp_mmsg::IoVec {
                // SAFETY: the slab holds UDP_RECV_BATCH slots of UDP_RECV_SLOT bytes.
                base: unsafe { base.add(slot * UDP_RECV_SLOT) }.cast(),
                len: UDP_RECV_SLOT,
            });
        let iov_base = iov.as_mut_ptr();
        let mut msgs: [udp_mmsg::MMsgHdr; UDP_RECV_BATCH] =
            std::array::from_fn(|slot| udp_mmsg::MMsgHdr {
                hdr: udp_mmsg::MsgHdr {
                    name: std::ptr::null_mut(),
                    name_len: 0,
                    // SAFETY: `slot < UDP_RECV_BATCH`, within `iov`.
                    iov: unsafe { iov_base.add(slot) },
                    iov_len: 1,
                    control: std::ptr::null_mut(),
                    control_len: 0,
                    flags: 0,
                },
                len: 0,
            });
        // SAFETY: every header points at a live iovec over a distinct,
        // exclusively borrowed slab slot; the socket is connected, so no
        // source address buffer is needed.
        let received = unsafe {
            udp_mmsg::recvmmsg(
                self.socket.as_raw_fd(),
                msgs.as_mut_ptr(),
                UDP_RECV_BATCH as std::ffi::c_uint,
                udp_mmsg::MSG_WAITFORONE,
                std::ptr::null_mut(),
            )
        };
        let received_at = std::time::Instant::now();
        let received = usize::try_from(received).map_err(|_| io::Error::last_os_error())?;
        state.received_at = received_at;
        for (len, msg) in state.lens.iter_mut().zip(&msgs[..received]) {
            *len = msg.len as usize;
        }
        state.filled = received;
        Ok(())
    }

    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    fn fill(&self, state: &mut UdpRecvState) -> io::Result<()> {
        #[cfg(test)]
        self.recv_syscalls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        state.next = 0;
        state.filled = 0;
        let count = self.socket.recv(&mut state.slab[..UDP_RECV_SLOT])?;
        state.received_at = std::time::Instant::now();
        state.lens[0] = count;
        state.filled = 1;
        Ok(())
    }
}

#[cfg(test)]
mod udp_transport_tests {
    use super::*;
    use std::sync::atomic::Ordering::SeqCst;

    fn pair() -> (UdpSocket, UdpTransport) {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(peer.local_addr().unwrap()).unwrap();
        peer.connect(socket.local_addr().unwrap()).unwrap();
        (peer, UdpTransport::new(socket).unwrap())
    }

    fn take(transport: &UdpTransport) -> Vec<u8> {
        let (datagram, count) = transport.receive().unwrap();
        datagram[..count].to_vec()
    }

    #[test]
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    fn recvmmsg_layout_matches_linux_libc_headers() {
        // Verified against sys/socket.h and sys/uio.h on Omarchy x86_64.
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<udp_mmsg::IoVec>(), 16);
        assert_eq!(size_of::<udp_mmsg::MsgHdr>(), 56);
        assert_eq!(size_of::<udp_mmsg::MMsgHdr>(), 64);
        assert_eq!(offset_of!(udp_mmsg::MsgHdr, iov), 16);
        assert_eq!(offset_of!(udp_mmsg::MsgHdr, iov_len), 24);
        assert_eq!(offset_of!(udp_mmsg::MsgHdr, control), 32);
        assert_eq!(offset_of!(udp_mmsg::MsgHdr, control_len), 40);
        assert_eq!(offset_of!(udp_mmsg::MsgHdr, flags), 48);
        assert_eq!(offset_of!(udp_mmsg::MMsgHdr, len), 56);
    }

    #[test]
    fn queued_datagrams_are_returned_individually_in_order() {
        let (peer, transport) = pair();
        *transport.timeout.lock().unwrap() = Some(Duration::from_secs(5));
        let sent: Vec<Vec<u8>> = vec![vec![1; 3], vec![2; 4096], vec![3; 1], vec![4; 8192]];
        for datagram in &sent {
            peer.send(datagram).unwrap();
        }
        let mut arrivals = Vec::new();
        let received: Vec<Vec<u8>> = sent
            .iter()
            .map(|_| {
                let (datagram, count) = transport.receive().unwrap();
                arrivals.push(datagram.received_at());
                datagram[..count].to_vec()
            })
            .collect();
        assert_eq!(received, sent);
        assert_eq!(transport.alloc_count.load(SeqCst), 1);
        let syscalls = transport.recv_syscalls.load(SeqCst);
        if cfg!(all(target_os = "linux", target_pointer_width = "64")) {
            // Loopback delivery is synchronous, so one recvmmsg drains all four.
            assert_eq!(syscalls, 1);
            assert!(arrivals.iter().all(|at| *at == arrivals[0]));
            println!("recvmmsg calls for four datagrams: {syscalls}; shared batch arrival timestamp verified");
        } else {
            assert_eq!(syscalls, sent.len());
        }
    }

    #[test]
    fn unchanged_poll_slice_sets_socket_timeout_once() {
        let (peer, transport) = pair();
        *transport.timeout.lock().unwrap() = Some(Duration::from_secs(5));
        for round in 0..UDP_RECV_BATCH * 3 {
            peer.send(&[round as u8]).unwrap();
            assert_eq!(take(&transport), vec![round as u8]);
        }
        assert_eq!(transport.timeout_syscalls.load(SeqCst), 1);
        assert_eq!(
            transport.socket.read_timeout().unwrap(),
            Some(UDP_RECV_POLL_SLICE)
        );
    }

    #[test]
    fn short_fixed_timeout_sets_socket_timeout_once() {
        let (peer, transport) = pair();
        *transport.timeout.lock().unwrap() = Some(Duration::from_millis(20));
        for round in 0..UDP_RECV_BATCH * 3 {
            peer.send(&[round as u8]).unwrap();
            assert_eq!(take(&transport), vec![round as u8]);
        }
        assert_eq!(transport.timeout_syscalls.load(SeqCst), 1);
    }

    #[test]
    fn short_deadline_reapplies_shrunken_slice_and_times_out() {
        let (_peer, transport) = pair();
        *transport.timeout.lock().unwrap() = Some(Duration::from_millis(10));
        let error = transport.receive().err().expect("no datagram was sent");
        assert!(matches!(&error, SessionError::Io(io) if io.kind() == io::ErrorKind::TimedOut));
        assert!(transport.timeout_syscalls.load(SeqCst) >= 1);
        assert!(transport.socket.read_timeout().unwrap().unwrap() <= Duration::from_millis(10));
    }

    #[test]
    fn cancellation_wins_over_queued_datagrams() {
        let (peer, transport) = pair();
        *transport.timeout.lock().unwrap() = Some(Duration::from_secs(5));
        peer.send(&[1]).unwrap();
        peer.send(&[2]).unwrap();
        assert_eq!(take(&transport), vec![1]);
        transport.cancel.send_replace(true);
        assert!(matches!(transport.receive(), Err(SessionError::NotReady)));
        transport.cancel.send_replace(false);
        assert_eq!(take(&transport), vec![2]);
    }
}

#[derive(Default)]
struct EventSlots {
    clipboard: Option<String>,
    ping: bool,
    /// Stream-configuration responses/rejects/errors, kept in arrival order so
    /// a caller of `request_stream_config` can correlate them by request id.
    stream_config: VecDeque<ControlMessage>,
    error: Option<SessionError>,
    closed: bool,
}

/// Retained stream-configuration results, oldest dropped past this bound.
const STREAM_CONFIG_SLOTS: usize = 16;

#[derive(Clone, Default)]
pub struct RuntimeEvents {
    shared: Arc<(Mutex<EventSlots>, Condvar)>,
}

impl RuntimeEvents {
    fn close(&self) {
        let (lock, ready) = &*self.shared;
        let mut slots = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slots.closed = true;
        ready.notify_all();
    }
    fn push(&self, event: Result<SessionEvent, SessionError>) {
        let (lock, ready) = &*self.shared;
        let mut slots = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match event {
            Ok(SessionEvent::Clipboard(text)) => slots.clipboard = Some(text),
            Ok(SessionEvent::Ping) => slots.ping = true,
            Ok(SessionEvent::StreamConfig(message)) => {
                if slots.stream_config.len() >= STREAM_CONFIG_SLOTS {
                    slots.stream_config.pop_front();
                }
                slots.stream_config.push_back(message);
            }
            Err(error) => slots.error = Some(error),
            Ok(
                SessionEvent::Ignored
                | SessionEvent::Frame(_)
                | SessionEvent::Audio(_)
                | SessionEvent::Cursor(_)
                | SessionEvent::InputAck { .. },
            ) => {}
        }
        ready.notify_one();
    }

    fn take(slots: &mut EventSlots) -> Option<Result<SessionEvent, SessionError>> {
        if let Some(text) = slots.clipboard.take() {
            return Some(Ok(SessionEvent::Clipboard(text)));
        }
        if std::mem::take(&mut slots.ping) {
            return Some(Ok(SessionEvent::Ping));
        }
        if let Some(message) = slots.stream_config.pop_front() {
            return Some(Ok(SessionEvent::StreamConfig(message)));
        }
        slots.error.take().map(Err)
    }

    fn pending(slots: &EventSlots) -> bool {
        slots.clipboard.is_some()
            || slots.ping
            || !slots.stream_config.is_empty()
            || slots.error.is_some()
    }

    pub fn try_recv(&self) -> Result<Result<SessionEvent, SessionError>, mpsc::TryRecvError> {
        let mut slots = self
            .shared
            .0
            .lock()
            .map_err(|_| mpsc::TryRecvError::Disconnected)?;
        Self::take(&mut slots).ok_or(if slots.closed {
            mpsc::TryRecvError::Disconnected
        } else {
            mpsc::TryRecvError::Empty
        })
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Result<SessionEvent, SessionError>, mpsc::RecvTimeoutError> {
        let (lock, ready) = &*self.shared;
        let slots = lock
            .lock()
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;
        let (mut slots, _) = ready
            .wait_timeout_while(slots, timeout, |slots| {
                !Self::pending(slots) && !slots.closed
            })
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;
        Self::take(&mut slots).ok_or(if slots.closed {
            mpsc::RecvTimeoutError::Disconnected
        } else {
            mpsc::RecvTimeoutError::Timeout
        })
    }

    pub fn recv(&self) -> Result<Result<SessionEvent, SessionError>, mpsc::RecvError> {
        let (lock, ready) = &*self.shared;
        let slots = lock.lock().map_err(|_| mpsc::RecvError)?;
        let mut slots = ready
            .wait_while(slots, |slots| !Self::pending(slots) && !slots.closed)
            .map_err(|_| mpsc::RecvError)?;
        Self::take(&mut slots).ok_or(mpsc::RecvError)
    }
}

pub struct SessionRuntime {
    events: RuntimeEvents,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
    /// Supervisor interrupt, used only to abort an in-flight automatic
    /// reconnect; the flag is set while that reconnect is running.
    interrupt: Option<(SessionInterruptHandle, Arc<AtomicBool>)>,
}

impl SessionRuntime {
    pub fn events(&self) -> &RuntimeEvents {
        &self.events
    }

    #[cfg(test)]
    fn from_worker_for_test(
        worker: thread::JoinHandle<()>,
        stop: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Self {
        Self {
            events: RuntimeEvents::default(),
            stop,
            worker: Some(worker),
            interrupt: None,
        }
    }

    pub fn stop(&mut self) -> Result<(), SessionError> {
        // Stopping the runtime must not cancel the session itself: callers
        // reconnect the same ClientSession after a stop (worker-respawn
        // recovery). Interrupt only to abort an in-flight automatic reconnect,
        // and restore the cancel flag afterwards unless it was already set.
        let mut restore = None;
        if let Some((interrupt, reconnecting)) = self.interrupt.take() {
            if reconnecting.load(Ordering::SeqCst) && !interrupt.is_cancelled() {
                interrupt.interrupt();
                restore = Some(interrupt);
            }
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let joined = self.worker.take().map(|worker| worker.join());
        if let Some(interrupt) = restore {
            interrupt.cancelled.store(false, Ordering::SeqCst);
            interrupt.cancel_signal.send_if_modified(|cancelled| {
                *cancelled = false;
                false
            });
        }
        if let Some(Err(payload)) = joined {
            self.events.close();
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|message| (*message).to_owned())
                })
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            return Err(SessionError::TcpRuntimePanicked(message));
        }
        Ok(())
    }
}

impl Drop for SessionRuntime {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn resolve_one((host, port): (&str, u16)) -> Result<SocketAddr, SessionError> {
    ferryx_rd_net::discovery::resolve_socket_addrs(host, port)?
        .into_iter()
        .next()
        .ok_or(SessionError::NoAddress)
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl From<crate::MediaAssemblyError> for SessionError {
    fn from(error: crate::MediaAssemblyError) -> Self {
        SessionError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            error.to_string(),
        ))
    }
}

#[cfg(test)]
mod owned_abr_tests {
    use super::*;

    #[test]
    fn progress_reports_resolution_before_failure() {
        let session =
            ClientSession::new(SessionConfig::direct("fe80::1%maho-no-such-if", "test")).unwrap();
        let phases = Arc::new(Mutex::new(Vec::new()));
        let observed = phases.clone();
        session.set_progress_observer(move |phase| observed.lock().unwrap().push(phase));
        assert!(session.pair_with_pin("00123456").is_err());
        let phases = phases.lock().unwrap();
        assert_eq!(phases[0], ConnectPhase::Resolving);
        assert!(
            matches!(phases.last(), Some(ConnectPhase::Failed { reason }) if !reason.is_empty())
        );
        assert_eq!(phases.len(), 2);
    }

    #[test]
    fn resolve_one_rejects_unknown_ipv6_scope_interface() {
        let error = resolve_one(("fe80::1%maho-no-such-if", 1)).unwrap_err();
        assert!(
            matches!(error, SessionError::Io(ref e) if e.kind() == io::ErrorKind::InvalidInput)
        );
        assert_eq!(
            resolve_one(("127.0.0.1", 7)).unwrap(),
            "127.0.0.1:7".parse().unwrap()
        );
    }

    #[test]
    fn confirmed_rate_seeds_controller_and_respects_user_cap() {
        let mut state = SessionStateInner::default();
        assert!(state.abr.is_none());
        state.confirm_abr(8_000_000);
        assert_eq!(state.abr.as_ref().unwrap().current_bitrate(), 8_000_000);
        state.abr_bounds = Some((500_000, 6_000_000));
        state.confirm_abr(8_000_000);
        assert_eq!(state.abr.as_ref().unwrap().current_bitrate(), 6_000_000);
        state.confirm_abr(0);
        assert_eq!(state.abr.as_ref().unwrap().current_bitrate(), 6_000_000);
    }

    #[test]
    fn owned_tick_emits_bitrate_adjust_on_the_real_control_channel() {
        let psk = PskIdentity::pairing("abr", &[7; 32]).unwrap();
        let listener = ferryx_rd_net::TlsPskServer::new([psk.clone()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (control_tx, control_rx) = mpsc::channel();
        let host = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let frame = stream.read_frame().unwrap();
            control_tx
                .send(ControlMessage::decode(&frame[PacketHeader::SIZE..]).unwrap())
                .unwrap();
        });
        let stream = TlsPskClient::new(psk).unwrap().connect(address).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "abr");
        config.pairing_store_path = Some(dir.path().join("pairing.json"));
        let session = ClientSession::new(config).unwrap();
        *session.tcp.lock().unwrap() = Some(stream);
        let now = std::time::Instant::now();
        {
            let mut state = session.state.lock().unwrap();
            state.state = SessionState::Ready;
            state.confirm_abr(8_000_000);
            // Header gaps are genuine receiver loss, not a mocked ABR result.
            for id in [0, 10] {
                state
                    .frames
                    .push_header(
                        FrameHeader {
                            frame_id: id,
                            width: 2,
                            height: 2,
                            is_key_frame: false,
                            total_chunks: 1,
                            total_size: 1,
                        },
                        id * 16,
                        now,
                    )
                    .unwrap();
            }
        }
        assert_eq!(session.tick_abr(now).unwrap(), Some(6_000_000));
        assert_eq!(
            control_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ControlMessage::BitrateAdjust(BitrateAdjust {
                target_bitrate: 6_000_000
            })
        );
        assert_eq!(
            session
                .receiver_snapshot_at(now)
                .unwrap()
                .abr_target_bitrate,
            Some(6_000_000)
        );
        assert_eq!(session.tick_abr(now).unwrap(), None);
        host.join().unwrap();
    }

    #[test]
    fn invalid_user_bounds_are_rejected_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "abr-test");
        config.pairing_store_path = Some(dir.path().join("pairing.json"));
        let session = ClientSession::new(config).unwrap();
        assert!(session.configure_abr(0, 8_000_000).is_err());
        assert!(session.configure_abr(8_000_000, 1_000_000).is_err());
        session.configure_abr(500_000, 8_000_000).unwrap();
        assert_eq!(
            session.state.lock().unwrap().abr_bounds,
            Some((500_000, 8_000_000))
        );
        assert_eq!(session.tick_abr(std::time::Instant::now()).unwrap(), None);
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

    use ferryx_rd_net::TlsPskServer;
    use ferryx_rd_proto::{HostDisplayList, StreamCodecPreference};
    fn parity_session() -> ClientSession {
        let mut config = SessionConfig::direct("127.0.0.1", "test");
        config.pairing_store_path = Some(PathBuf::from("/unused-parity-fixture"));
        let session = ClientSession::new(config).unwrap();
        {
            let mut state = session.state.lock().unwrap();
            state.state = SessionState::Ready;
            state.server = Some(Handshake {
                name: "fixture".into(),
                width: 1,
                height: 1,
                scale: 1.0,
                version: PROTOCOL_VERSION,
                capabilities: SessionConfig::implemented_capabilities(),
                pairing_id: String::new(),
                session_salt: [0; 16],
            });
        }
        session
    }

    #[test]
    fn negotiated_permissions_deny_before_initial_grant() {
        let session = parity_session();
        let p = session.effective_permissions().unwrap().unwrap();
        assert!(!p.keyboard && !p.mouse && !p.clipboard && !p.audio && !p.gamepad);
        assert!(matches!(
            session.send_input(InputEvent::unicode_char(65, 0.0, 0.0)),
            Err(SessionError::PermissionDenied)
        ));
    }

    #[test]
    fn clipboard_gate_allows_policy_less_host_and_honours_negotiated_permissions() {
        // A pre-policy host: no GUEST_PERMISSIONS and never sends HostPolicy.
        let session = parity_session();
        session
            .state
            .lock()
            .unwrap()
            .server
            .as_mut()
            .unwrap()
            .capabilities =
            Capabilities::AUTHENTICATED_UDP_REGISTRATION | Capabilities::STREAM_OPTIONS;
        assert_eq!(session.effective_permissions().unwrap(), None);
        assert!(session.clipboard_allowed().unwrap());

        // A HostPolicy from such a host is honoured both ways.
        session
            .handle_packet(
                PacketHeader::new(PacketType::Control, 1, 0, 0),
                ControlMessage::HostPolicy(HostPolicy {
                    allow_input: true,
                    allow_clipboard: false,
                })
                .encode()
                .unwrap(),
                None,
            )
            .unwrap();
        assert!(!session.clipboard_allowed().unwrap());
        assert!(matches!(
            session.send_clipboard_text("x".into()),
            Err(SessionError::PermissionDenied)
        ));

        // Negotiated GuestPermissions stay authoritative: denied until granted.
        let negotiated = parity_session();
        assert!(!negotiated.clipboard_allowed().unwrap());
        negotiated
            .handle_packet(
                PacketHeader::new(PacketType::Control, 1, 0, 0),
                ControlMessage::GuestPermissions(ferryx_rd_proto::GuestPermissions {
                    keyboard: false,
                    mouse: false,
                    gamepad: false,
                    clipboard: true,
                    audio: false,
                })
                .encode()
                .unwrap(),
                None,
            )
            .unwrap();
        assert!(negotiated.clipboard_allowed().unwrap());
    }

    #[test]
    fn effective_permissions_supersede_later_host_policy() {
        let session = parity_session();
        let permissions = ferryx_rd_proto::GuestPermissions {
            keyboard: true,
            mouse: false,
            gamepad: false,
            clipboard: true,
            audio: false,
        };
        session
            .handle_packet(
                PacketHeader::new(PacketType::Control, 1, 0, 0),
                ControlMessage::GuestPermissions(permissions)
                    .encode()
                    .unwrap(),
                None,
            )
            .unwrap();
        session
            .handle_packet(
                PacketHeader::new(PacketType::Control, 2, 0, 0),
                ControlMessage::HostPolicy(HostPolicy {
                    allow_input: true,
                    allow_clipboard: false,
                })
                .encode()
                .unwrap(),
                None,
            )
            .unwrap();
        assert_eq!(session.effective_permissions().unwrap(), Some(permissions));
        assert_eq!(
            session.stream_state().unwrap().policy,
            Some(HostPolicy {
                allow_input: false,
                allow_clipboard: true
            })
        );
    }

    #[test]
    fn unnegotiated_host_message_is_rejected() {
        let session = parity_session();
        session
            .state
            .lock()
            .unwrap()
            .server
            .as_mut()
            .unwrap()
            .capabilities = Capabilities::AUTHENTICATED_UDP_REGISTRATION;
        let message = ControlMessage::HostStatus(ferryx_rd_proto::HostStatus {
            guest_names: vec![],
        });
        assert!(matches!(
            session.handle_packet(
                PacketHeader::new(PacketType::Control, 0, 0, 0),
                message.encode().unwrap(),
                None
            ),
            Err(SessionError::Unsupported)
        ));
    }

    #[test]
    fn secure_attention_response_requires_outstanding_id() {
        let session = parity_session();
        session.state.lock().unwrap().parity.sas_pending = Some(4);
        let response = ferryx_rd_proto::SecureAttentionResponse {
            request_id: 3,
            result: ferryx_rd_proto::SecureAttentionResult::Accepted,
        };
        session
            .handle_packet(
                PacketHeader::new(PacketType::Control, 0, 0, 0),
                ControlMessage::SecureAttentionResponse(response)
                    .encode()
                    .unwrap(),
                None,
            )
            .unwrap();
        assert_eq!(
            session.session_info().unwrap().secure_attention_response,
            None
        );
    }
    fn packet(packet_type: PacketType, payload: &[u8]) -> Vec<u8> {
        let mut packet = PacketHeader::new(packet_type, 0, 0, 0).encode().unwrap();
        packet.extend_from_slice(payload);
        packet
    }
    fn split_packet(packet: &[u8]) -> (PacketHeader, &[u8]) {
        (
            PacketHeader::decode(&packet[..PacketHeader::SIZE]).unwrap(),
            &packet[PacketHeader::SIZE..],
        )
    }
    #[test]
    fn session_runtime_stop_propagates_worker_panic() {
        let (started_tx, started_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            started_tx.send(()).unwrap();
            panic!("simulated worker panic in session runtime");
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut runtime = crate::SessionRuntime::from_worker_for_test(worker, None);
        let error = runtime.stop().expect_err("worker panic must be returned");
        assert!(matches!(error, SessionError::TcpRuntimePanicked(message)
        if message == "simulated worker panic in session runtime"));
        runtime.stop().unwrap();
        assert!(matches!(
            runtime.events().try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn runtime_events_retain_stream_configuration_results() {
        let events = RuntimeEvents::default();
        let response =
            ControlMessage::StreamConfigResponse(ferryx_rd_proto::StreamConfigurationResponse {
                request_id: 7,
                active: ferryx_rd_proto::StreamConfiguration {
                    width: 1920,
                    height: 1080,
                    bitrate: 8_000_000,
                    frames_per_second: 60,
                },
            });
        let reject = ControlMessage::StreamConfigReject(ferryx_rd_proto::StreamConfigurationReject {
            request_id: 8,
            reason: ferryx_rd_proto::StreamConfigurationErrorCode::UnsupportedDimensions,
            message: "nope".to_owned(),
        });
        events.push(Ok(SessionEvent::StreamConfig(response.clone())));
        events.push(Ok(SessionEvent::StreamConfig(reject.clone())));

        assert_eq!(
            events
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap(),
            SessionEvent::StreamConfig(response)
        );
        assert_eq!(
            events.try_recv().unwrap().unwrap(),
            SessionEvent::StreamConfig(reject)
        );
        assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn session_stream_options_events_update_stream_state() {
        let session =
            ClientSession::new(SessionConfig::direct("127.0.0.1", "test-client")).unwrap();
        assert_eq!(session.stream_state().unwrap(), StreamState::default());
        // Control packets are only handled once the handshake has completed.
        session.state.lock().unwrap().state = SessionState::Ready;
        session.state.lock().unwrap().server = Some(Handshake {
            name: "fixture".into(),
            width: 1,
            height: 1,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::STREAM_OPTIONS,
            pairing_id: String::new(),
            session_salt: [0; 16],
        });

        // HostPolicy event
        let policy = HostPolicy {
            allow_input: false,
            allow_clipboard: true,
        };
        let msg = ControlMessage::HostPolicy(policy);
        let header = PacketHeader::new(PacketType::Control, 1, 0, 0);
        let event = session
            .handle_packet(header, msg.encode().unwrap(), None)
            .unwrap();
        assert_eq!(
            event,
            SessionEvent::StreamConfig(ControlMessage::HostPolicy(policy))
        );
        assert_eq!(session.stream_state().unwrap().policy, Some(policy));

        // HostDisplayList event
        let displays = vec![
            HostDisplay {
                index: 0,
                width: 3840,
                height: 1600,
                primary: true,
                name: "DP-1".to_string(),
            },
            HostDisplay {
                index: 1,
                width: 1920,
                height: 1080,
                primary: false,
                name: "HDMI-1".to_string(),
            },
        ];
        let display_list = HostDisplayList {
            displays: displays.clone(),
        };
        let msg = ControlMessage::HostDisplayList(display_list.clone());
        let header = PacketHeader::new(PacketType::Control, 2, 0, 0);
        let event = session
            .handle_packet(header, msg.encode().unwrap(), None)
            .unwrap();
        assert_eq!(
            event,
            SessionEvent::StreamConfig(ControlMessage::HostDisplayList(display_list))
        );
        assert_eq!(session.stream_state().unwrap().displays, displays);

        // StreamOptionsActive event
        let active = StreamOptionsActive {
            request_id: 42,
            width: 1920,
            height: 800,
            fps: 120,
            codec: StreamCodecPreference::Hevc,
            display_index: 0,
            bitrate: 15_000_000,
        };
        let msg = ControlMessage::StreamOptionsActive(active);
        let header = PacketHeader::new(PacketType::Control, 3, 0, 0);
        let event = session
            .handle_packet(header, msg.encode().unwrap(), None)
            .unwrap();
        assert_eq!(
            event,
            SessionEvent::StreamConfig(ControlMessage::StreamOptionsActive(active))
        );
        assert_eq!(session.stream_state().unwrap().active, Some(active));

        // StreamConfigReject event
        let reject = ferryx_rd_proto::StreamConfigurationReject {
            request_id: 43,
            reason: ferryx_rd_proto::StreamConfigurationErrorCode::UnsupportedFps,
            message: "fps too high".to_string(),
        };
        let msg = ControlMessage::StreamConfigReject(reject.clone());
        let header = PacketHeader::new(PacketType::Control, 4, 0, 0);
        let event = session
            .handle_packet(header, msg.encode().unwrap(), None)
            .unwrap();
        assert_eq!(
            event,
            SessionEvent::StreamConfig(ControlMessage::StreamConfigReject(reject))
        );
        assert_eq!(
            session.stream_state().unwrap().last_reject,
            Some("fps too high".to_string())
        );

        // Host supports stream options capability test
        let mut session_state = session.state.lock().unwrap();
        session_state.stream_state.host_supports_stream_options = true;
        drop(session_state);
        assert!(session.stream_state().unwrap().host_supports_stream_options);
    }

    #[test]
    fn client_session_udp_receive_reuses_buffer_across_cancellations() {
        let pairing_id = "udp-reuse-pairing-id";
        let key = [0x55; 32];
        let psk = PskIdentity::pairing(pairing_id, &key).unwrap();
        let listener = TlsPskServer::new([psk])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let tcp_address = listener.local_addr().unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_port = udp.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let handshake_packet = stream.read_frame().unwrap();
            let (header, payload) = split_packet(&handshake_packet);
            assert_eq!(header.packet_type, PacketType::Handshake);
            let handshake = Handshake::decode(payload).unwrap();
            let salt = handshake.session_salt;

            let acknowledgement = Handshake {
                name: "mock-host".to_owned(),
                width: 1920,
                height: 1080,
                scale: 1.0,
                version: PROTOCOL_VERSION,
                capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
                pairing_id: String::new(),
                session_salt: [0; 16],
            };
            stream
                .write_frame(&packet(
                    PacketType::HandshakeAck,
                    &acknowledgement.encode().unwrap(),
                ))
                .unwrap();

            let mut ping = [0_u8; 1024];
            let (reg_len, client_udp_addr) = udp.recv_from(&mut ping).unwrap();
            let mut c2h = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
            let (reg_hdr, _) = c2h.open_datagram(&ping[..reg_len]).unwrap();
            assert_eq!(reg_hdr.packet_type, PacketType::Ping);

            (udp, client_udp_addr, key, salt, stream)
        });

        let config = SessionConfig {
            host: "127.0.0.1".to_owned(),
            tcp_port: tcp_address.port(),
            udp_port,
            client_name: "rust-client".to_owned(),
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_store_path: None,
            connect_timeout: Duration::from_secs(2),
            handshake_ack_timeout: Duration::from_secs(2),
        };
        let session = ClientSession::new(config).unwrap();
        let record = crate::PairingRecord {
            id: pairing_id.to_string(),
            name: "mock-host".to_string(),
            key: key.to_vec(),
            added_at_unix_ms: 0,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        };
        let phases = Arc::new(Mutex::new(Vec::new()));
        let observed = phases.clone();
        session.set_progress_observer(move |phase| observed.lock().unwrap().push(phase));
        let ready = session.connect_with_pairing(record).unwrap();
        assert_eq!(ready.server.name, "mock-host");
        assert_eq!(
            *phases.lock().unwrap(),
            vec![
                ConnectPhase::Resolving,
                ConnectPhase::Connecting,
                ConnectPhase::Authenticating,
                ConnectPhase::StartingVideo
            ]
        );
        session.video_ready();
        assert_eq!(phases.lock().unwrap().last(), Some(&ConnectPhase::Ready));

        // Attempt 1: Cancelled receive
        let (entered_tx1, entered_rx1) = mpsc::channel();
        session.register_udp_entered_hook_for_test(entered_tx1);
        let s1 = session.clone();
        let (res_tx1, res_rx1) = mpsc::channel();
        let t1 = thread::spawn(move || {
            let res = s1.receive_udp_event();
            res_tx1.send(res).unwrap();
        });
        entered_rx1.recv_timeout(Duration::from_secs(5)).unwrap();
        let ptr1 = session
            .udp_recv_buffer_ptr()
            .expect("buffer pointer must exist");
        session.cancel_udp_receive_for_test();
        let res1 = res_rx1.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(res1, Err(SessionError::NotReady)));
        t1.join().unwrap();
        session.reset_udp_cancel_for_test();

        // Attempt 2: Cancelled receive
        let (entered_tx2, entered_rx2) = mpsc::channel();
        session.register_udp_entered_hook_for_test(entered_tx2);
        let s2 = session.clone();
        let (res_tx2, res_rx2) = mpsc::channel();
        let t2 = thread::spawn(move || {
            let res = s2.receive_udp_event();
            res_tx2.send(res).unwrap();
        });
        entered_rx2.recv_timeout(Duration::from_secs(5)).unwrap();
        let ptr2 = session
            .udp_recv_buffer_ptr()
            .expect("buffer pointer must exist");
        session.cancel_udp_receive_for_test();
        let res2 = res_rx2.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(res2, Err(SessionError::NotReady)));
        t2.join().unwrap();
        session.reset_udp_cancel_for_test();

        // Attempt 3: Successful datagram receive
        let (server_udp, client_udp_addr, server_key, session_salt, _tcp_stream) =
            server.join().unwrap();
        let mut server_cipher = ferryx_rd_net::DatagramCipher::derive(
            &server_key,
            &session_salt,
            ferryx_rd_net::Direction::HostToClient,
        )
        .unwrap();
        let ping_header = PacketHeader::new(PacketType::Ping, 1, 0, 0);
        let ping_datagram = server_cipher.seal_datagram(&ping_header, &[]).unwrap();

        let (entered_tx3, entered_rx3) = mpsc::channel();
        session.register_udp_entered_hook_for_test(entered_tx3);
        let s3 = session.clone();
        let (res_tx3, res_rx3) = mpsc::channel();
        let t3 = thread::spawn(move || {
            let res = s3.receive_udp_event();
            res_tx3.send(res).unwrap();
        });
        entered_rx3.recv_timeout(Duration::from_secs(5)).unwrap();
        let ptr3 = session
            .udp_recv_buffer_ptr()
            .expect("buffer pointer must exist");
        server_udp.send_to(&ping_datagram, client_udp_addr).unwrap();
        let res3 = res_rx3.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(res3.unwrap(), crate::SessionEvent::Ping);
        t3.join().unwrap();

        // A longer packet after a short packet must still have the full receive capacity.
        let header = PacketHeader::new(PacketType::Ping, 2, 0, 0);
        let datagram = server_cipher.seal_datagram(&header, &[7; 4096]).unwrap();
        server_udp.send_to(&datagram, client_udp_addr).unwrap();
        assert_eq!(
            session.receive_udp_event().unwrap(),
            crate::SessionEvent::Ping
        );

        // Deterministic assertions:
        let alloc_count = session.udp_recv_alloc_count().unwrap();
        assert_eq!(
            alloc_count, 1,
            "expected single 65536-byte receive allocation across attempts, but got {}",
            alloc_count
        );
        assert_eq!(
            ptr1, ptr2,
            "buffer pointer must be identical across cancellation attempts"
        );
        assert_eq!(
            ptr2, ptr3,
            "buffer pointer must be identical between cancellation and success"
        );

        session.disconnect().unwrap();
    }

    #[test]
    fn udp_cancel_wakes_registered_receive() {
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(peer.local_addr().unwrap()).unwrap();
        let transport = Arc::new(UdpTransport::new(socket).unwrap());
        let temporary = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "udp");
        config.pairing_store_path = Some(temporary.path().join("pairings.json"));
        let session = ClientSession::new(config).unwrap();
        *session.udp.lock().unwrap() = Some(transport.clone());
        session.state.lock().unwrap().state = SessionState::Ready;
        let receiver = session.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        *transport.entered.lock().unwrap() = Some(entered_tx);
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let result = receiver.receive_udp_event();
            done_tx
                .send(matches!(result, Err(SessionError::NotReady)))
                .unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        session.disconnect().unwrap();
        assert!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn tls_read_step_returns_with_partial_frame() {
        use std::io::Write;
        let psk = PskIdentity::pairing("step", &[9; 32]).unwrap();
        let listener = ferryx_rd_net::TlsPskServer::new([psk.clone()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (partial_tx, partial_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let host = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream.ssl_stream_mut().write_all(&[20, 0]).unwrap();
            partial_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let mut stream = TlsPskClient::new(psk).unwrap().connect(address).unwrap();
        partial_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        stream
            .ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let result = stream.read_frame_step();
        release_tx.send(()).unwrap();
        host.join().unwrap();
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn runtime_input_and_stop_progress_with_partial_frame() {
        use std::io::Write;
        let psk = PskIdentity::pairing("runtime", &[9; 32]).unwrap();
        let listener = ferryx_rd_net::TlsPskServer::new([psk.clone()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (partial_tx, partial_rx) = mpsc::channel();
        let (input_tx, input_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let host = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.ssl_stream_mut().write_all(&[20, 0]).unwrap();
            partial_tx.send(()).unwrap();
            let input = stream.read_frame().unwrap();
            assert_eq!(
                PacketHeader::decode(&input[..PacketHeader::SIZE])
                    .unwrap()
                    .packet_type,
                PacketType::InputEvent
            );
            input_tx
                .send(InputEvent::decode(&input[PacketHeader::SIZE..]).unwrap())
                .unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let stream = TlsPskClient::new(psk).unwrap().connect(address).unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "runtime");
        config.pairing_store_path = Some(temporary.path().join("pairings.json"));
        let session = ClientSession::new(config).unwrap();
        *session.tcp.lock().unwrap() = Some(stream);
        session.state.lock().unwrap().state = SessionState::Ready;
        let (wait_tx, wait_rx) = mpsc::channel();
        *session.tcp_wait.lock().unwrap() = Some(wait_tx);
        partial_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut runtime = session.spawn_tcp_runtime().unwrap();
        wait_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let input = InputEvent {
            event_type: ferryx_rd_proto::InputEventType::MouseMove,
            x: 0.25,
            y: 0.75,
            key_code: 0,
            modifiers: ferryx_rd_proto::Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        session.send_input(input).unwrap();
        assert_eq!(
            input_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            input
        );
        let (stop_tx, stop_rx) = mpsc::channel();
        let stop = thread::spawn(move || {
            runtime.stop().unwrap();
            stop_tx.send(()).unwrap();
        });
        stop_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        release_tx.send(()).unwrap();
        stop.join().unwrap();
        host.join().unwrap();
        session.disconnect().unwrap();
    }

    #[test]
    fn client_session_tracks_input_ack_events() {
        let config = SessionConfig::direct("127.0.0.1", "test-client");
        let session = ClientSession::new(config).unwrap();
        assert!(session.last_input_ack().is_none());

        // Simulate recording an InputAck via internal lock
        if let Ok(mut lock) = session.last_input_ack.lock() {
            *lock = Some((101, true, 0));
        }

        assert_eq!(session.last_input_ack(), Some((101, true, 0)));
    }

    #[test]
    fn worker_loss_is_retryable_but_real_failures_are_not() {
        // Given: the host service replaced its session worker, tearing down the
        // TCP control channel and the UDP media session under a connected
        // client. That is the Windows secure-desktop switch, and re-handshaking
        // with the stored pairing recovers it.
        for error in [
            SessionError::NotReady,
            SessionError::TcpRuntimeStopped,
            SessionError::TcpRuntimePanicked("worker exited".into()),
            SessionError::HandshakeAckTimeout,
            SessionError::Tls(TlsPskError::Io(io::Error::other("unclean TLS EOF"))),
        ] {
            assert!(
                should_reconnect(&error),
                "expected a reconnect for {error:?}"
            );
        }

        // Given: failures reconnecting cannot fix.
        for error in [
            SessionError::PairingNotFound("missing".into()),
            SessionError::NoAddress,
            SessionError::Tls(TlsPskError::Handshake("PSK rejected".into())),
            SessionError::Tls(TlsPskError::InvalidKeyLength),
            SessionError::PairingRejected(PairingRejectReason::DeniedByHost),
            SessionError::Io(io::Error::other("unrelated I/O failure")),
        ] {
            assert!(
                !should_reconnect(&error),
                "expected no reconnect for {error:?}"
            );
        }
    }
}

#[cfg(test)]
mod receiver_clock_tests {
    use super::*;
    use ferryx_rd_net::TlsPskServer;
    use std::time::Instant;

    #[test]
    fn explicit_clock_ingress_records_assembly_interval_on_that_clock() {
        let key = [0x39; 32];
        let listener = TlsPskServer::new([PskIdentity::pairing("clock", &key).unwrap()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let udp_peer = udp.try_clone().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "clock");
        config.tcp_port = listener.local_addr().unwrap().port();
        config.udp_port = udp.local_addr().unwrap().port();
        config.connect_timeout = Duration::from_secs(5);
        config.handshake_ack_timeout = Duration::from_secs(5);
        config.pairing_store_path = Some(temporary.path().join("pairings.json"));
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let frame = stream.read_frame().unwrap();
            let mut handshake = Handshake::decode(&frame[PacketHeader::SIZE..]).unwrap();
            handshake.capabilities = Capabilities::AUTHENTICATED_UDP_REGISTRATION;
            let mut ack = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            ack.extend_from_slice(&handshake.encode().unwrap());
            stream.write_frame(&ack).unwrap();
            let mut probe = [0; 1024];
            let (probe_len, peer) = udp.recv_from(&mut probe).unwrap();
            let c2h =
                DatagramCipher::derive(&key, &handshake.session_salt, Direction::ClientToHost)
                    .unwrap();
            let mut c2h = c2h;
            let (probe_hdr, _) = c2h.open_datagram(&probe[..probe_len]).unwrap();
            assert_eq!(probe_hdr.packet_type, PacketType::Ping);
            ready_tx.send((peer, handshake.session_salt)).unwrap();
            // Stay readable until the client disconnects at teardown.
            let _ = stream.read_frame();
        });
        let session = ClientSession::new(config).unwrap();
        session
            .connect_with_pairing(PairingRecord {
                id: "clock".into(),
                name: "fixture".into(),
                key: key.to_vec(),
                added_at_unix_ms: 0,
                last_endpoint: None,
                endpoint_aliases: Vec::new(),
                relay_url: None,
                relay_host_id: None,
                trust: ferryx_rd_proto::PairingTrust::Owner,
            })
            .unwrap();
        session
            .set_udp_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (peer, salt) = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut cipher = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let header_datagram = cipher
            .seal_datagram(
                &PacketHeader::new(PacketType::FrameHeader, 1, 0, 0),
                &FrameHeader {
                    frame_id: 7,
                    width: 1,
                    height: 1,
                    is_key_frame: true,
                    total_chunks: 1,
                    total_size: 1,
                }
                .encode()
                .unwrap(),
            )
            .unwrap();
        let chunk_datagram = cipher
            .seal_datagram(
                &PacketHeader::new(PacketType::FrameChunk, 2, 0, 0),
                &FrameChunk {
                    frame_id: 7,
                    chunk_index: 0,
                    data: vec![1],
                }
                .encode()
                .unwrap(),
            )
            .unwrap();

        // Future logical instants: wall-clock completion sampling would produce
        // a reversed interval and silently drop the assembly sample.
        let start = Instant::now() + Duration::from_secs(3600);
        udp_peer.send_to(&header_datagram, peer).unwrap();
        assert_eq!(
            session.receive_udp_event_at(start).unwrap(),
            SessionEvent::Ignored
        );
        udp_peer.send_to(&chunk_datagram, peer).unwrap();
        assert!(matches!(
            session
                .receive_udp_event_at(start + Duration::from_millis(5))
                .unwrap(),
            SessionEvent::Frame(_)
        ));
        let snapshot = session
            .receiver_snapshot_at(start + Duration::from_millis(6))
            .unwrap();
        let assembly = snapshot
            .receive_assembly_us
            .expect("explicit-clock assembly interval must be recorded");
        assert!(
            assembly.max_us <= 5_000,
            "assembly interval must come from the supplied clock, got {assembly:?}"
        );
        session.disconnect().unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn send_udp_registration_emits_authenticated_ping_and_retry() {
        let key = [0x5a; 32];
        let listener = TlsPskServer::new([PskIdentity::pairing("burst", &key).unwrap()])
            .unwrap()
            .bind("127.0.0.1:0")
            .unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let mut config = SessionConfig::direct("127.0.0.1", "burst");
        config.tcp_port = listener.local_addr().unwrap().port();
        config.udp_port = udp.local_addr().unwrap().port();
        config.connect_timeout = Duration::from_secs(5);
        config.handshake_ack_timeout = Duration::from_secs(5);
        config.pairing_store_path = Some(temporary.path().join("pairings.json"));

        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            stream
                .ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let frame = stream.read_frame().unwrap();
            let mut handshake = Handshake::decode(&frame[PacketHeader::SIZE..]).unwrap();
            handshake.capabilities = Capabilities::AUTHENTICATED_UDP_REGISTRATION;
            let mut ack = PacketHeader::new(PacketType::HandshakeAck, 0, 0, 0)
                .encode()
                .unwrap();
            ack.extend_from_slice(&handshake.encode().unwrap());
            stream.write_frame(&ack).unwrap();
            ready_tx.send(handshake.session_salt).unwrap();
            let _ = stream.read_frame();
        });

        let session = ClientSession::new(config).unwrap();
        session
            .connect_with_pairing(PairingRecord {
                id: "burst".into(),
                name: "burst_fixture".into(),
                key: key.to_vec(),
                added_at_unix_ms: 0,
                last_endpoint: None,
                endpoint_aliases: Vec::new(),
                relay_url: None,
                relay_host_id: None,
                trust: ferryx_rd_proto::PairingTrust::Owner,
            })
            .unwrap();
        let salt = ready_rx.recv().unwrap();
        let mut c2h = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();

        // Initial registration packet was sent during connect
        let mut probe = [0u8; 1024];
        let (len, _) = udp.recv_from(&mut probe).unwrap();
        let (hdr, payload) = c2h.open_datagram(&probe[..len]).unwrap();
        assert_eq!(hdr.packet_type, PacketType::Ping);
        assert!(payload.is_empty());

        // Resending sends an additional valid registration ping with fresh nonce
        session.send_udp_registration().unwrap();
        let (len, _) = udp.recv_from(&mut probe).unwrap();
        let (hdr, payload) = c2h.open_datagram(&probe[..len]).unwrap();
        assert_eq!(hdr.packet_type, PacketType::Ping);
        assert!(payload.is_empty());

        session.disconnect().unwrap();
        worker.join().unwrap();
    }
}
