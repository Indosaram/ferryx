use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::guests::{self, GuestCommand, GuestRegistry, GuestSession, PendingRequest};
use ferryx_rd_net::{
    BootstrapLockout, DatagramCipher, Direction, PskIdentity, TlsPskError, TlsPskServer,
    BOOTSTRAP_IDENTITY, PAIRING_IDENTITY_PREFIX,
};
use ferryx_rd_proto::CursorShape;
use ferryx_rd_proto::{
    AudioFragmentHeader, BitrateAdjust, Capabilities, ControlMessage, CursorUpdate, FrameHeader,
    Handshake, HostDisplay, HostDisplayList, HostPolicy, InputAckMessage, InputEvent, PacketHeader,
    PacketType, PairingGrant, PairingReject, PairingRejectReason, PairingRequest,
    StreamCodecPreference, StreamConfiguration, StreamConfigurationErrorCode,
    StreamConfigurationReject, StreamConfigurationResponse, StreamOptionsActive, WireCodec,
    MAX_AUDIO_FRAGMENT_BYTES, MAX_HOST_DISPLAY_NAME_BYTES, MAX_VIDEO_CHUNK_BYTES, PROTOCOL_VERSION,
};
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use ferryx_rd_proto::{ClipboardSyncDirection, ClipboardSyncOrigin, ClipboardSyncUpdate};
use ferryx_rd_proto::{
    GuestPermissions, PairingTrust, PairingTrustGrant, SecureAttentionResponse, SessionEndReason,
    SessionEnded,
};
use openssl::base64;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[cfg(target_os = "linux")]
use crate::capture_linux::{CaptureConfig as LinuxCaptureConfig, LinuxCapture};
#[cfg(target_os = "macos")]
use crate::capture_macos::{CaptureConfig, CaptureEvent, CaptureFrame, ScreenCapture};
#[cfg(target_os = "windows")]
use crate::capture_windows::{CaptureError, WindowsCapture};
#[cfg(target_os = "linux")]
use crate::encode_linux::{
    EncoderConfig as LinuxEncoderConfig, LinuxVideoEncoder, VideoCodec as LinuxVideoCodec,
};
#[cfg(target_os = "macos")]
use crate::encode_vt::{EncoderConfig, EncoderOptions, InputKind, VideoToolboxEncoder, VtCodec};
#[cfg(target_os = "windows")]
use crate::encode_windows::{EncoderConfig, MediaFoundationEncoder, VideoCodec};
#[cfg(target_os = "linux")]
use crate::inject_linux::LinuxInputInjector;
#[cfg(target_os = "macos")]
use crate::inject_macos::InputInjector;
#[cfg(target_os = "windows")]
use crate::inject_windows::WindowsInputInjector;
#[cfg(target_os = "windows")]
use crate::windows_logic::TargetDisplay;

pub const DEFAULT_TCP_PORT: u16 = 19_730;
pub const DEFAULT_UDP_PORT: u16 = 19_731;
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);
pub const SESSION_STALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound on how long session teardown waits for the UDP sender thread to
/// exit before closing the session without it.
pub const SENDER_JOIN_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_CONCURRENT_CONNECTIONS: usize = 16;
pub const PAIRING_WINDOW: Duration = Duration::from_secs(300);
/// The shared codec remains available through both legacy host paths.
///
/// ```
/// let stats = ferryx_rd_host::TimestampStats {
///     frame_id: 7,
///     capture_us: 10,
///     encode_start_us: 20,
///     encode_end_us: 30,
///     send_us: 40,
/// };
/// let session_stats: ferryx_rd_host::session::TimestampStats = stats;
/// let shared_stats: ferryx_rd_proto::TimestampStats = session_stats;
/// let encode: fn(ferryx_rd_host::TimestampStats) -> Vec<u8> =
///     ferryx_rd_host::session::TimestampStats::encode;
/// let decode: fn(&[u8]) -> Option<ferryx_rd_host::TimestampStats> =
///     ferryx_rd_host::session::TimestampStats::decode;
/// let bytes = encode(shared_stats);
/// assert_eq!(bytes.len(), 42);
/// assert_eq!(ferryx_rd_host::TimestampStats::SIZE, 42);
/// assert_eq!(&bytes[..6], ferryx_rd_host::session::TIMESTAMP_STATS_MAGIC);
/// assert_eq!(ferryx_rd_host::session::TIMESTAMP_STATS_MAGIC, b"ERDTS1");
/// assert_eq!(decode(&bytes), Some(stats));
/// ```
pub use ferryx_rd_proto::{TimestampStats, TIMESTAMP_STATS_MAGIC};
const SWIFT_REFERENCE_DATE_OFFSET: f64 = 978_307_200.0;

/// Process-lifetime stream-server counters, read by the session worker's
/// recycle watchdog: a worker spawned during desktop churn can accept TCP
/// forever while no TLS handshake ever completes, wedging the logon screen.
pub static SERVER_ACCEPTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static SERVER_TLS_SUCCESSES: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
pub static SERVER_LOOP_ITERATIONS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Pure session activity watch used by the watchdog thread to detect stalled
/// sessions. It does not hold sockets or streams, making it trivially unit-testable.
pub(crate) struct SessionWatch {
    origin: Instant,
    progress_ms: AtomicU64,
    timeout: Duration,
    aborted: AtomicBool,
}

impl SessionWatch {
    pub(crate) fn new(timeout: Duration) -> Self {
        let origin = Instant::now();
        Self {
            origin,
            progress_ms: AtomicU64::new(0),
            timeout,
            aborted: AtomicBool::new(false),
        }
    }

    pub(crate) fn tick(&self, now: Instant) {
        let elapsed_ms = now.saturating_duration_since(self.origin).as_millis() as u64;
        self.progress_ms.store(elapsed_ms, Ordering::Relaxed);
    }

    pub(crate) fn idle(&self, now: Instant) -> Duration {
        let now_ms = now.saturating_duration_since(self.origin).as_millis() as u64;
        let last_ms = self.progress_ms.load(Ordering::Relaxed);
        Duration::from_millis(now_ms.saturating_sub(last_ms))
    }

    pub(crate) fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    pub(crate) fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }
}

/// Pure decision predicate for the session stall watchdog.
pub(crate) fn watchdog_should_abort(finished: bool, idle: Duration, timeout: Duration) -> bool {
    !finished && idle >= timeout
}

/// Waits for a worker that is expected to exit promptly, without letting a
/// wedged worker hold session teardown open forever.
///
/// Returns `false` when the deadline expires; the caller closes the session
/// anyway, leaving that one worker detached rather than hanging the daemon.
fn join_with_deadline<T>(handle: thread::JoinHandle<T>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
    handle.join().is_ok()
}

struct ConnectionPermit(Arc<AtomicUsize>);

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct FinishedGuard(Arc<AtomicBool>);

impl Drop for FinishedGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
#[path = "host_trace.rs"]
pub(crate) mod host_trace;

#[cfg(any(target_os = "windows", test))]
pub(crate) fn capture_failure_from_label(label: &str) -> crate::windows_session::CaptureFailure {
    use crate::windows_session::CaptureFailure;

    match label {
        "access_lost" => CaptureFailure::AccessLost,
        "access_denied" => CaptureFailure::AccessDenied,
        "refresh_failure" => CaptureFailure::RefreshFailure,
        _ => CaptureFailure::Other,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRecord {
    pub id: String,
    pub name: String,
    pub key: [u8; 32],
    pub added_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PairingStore {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskPairingRecord {
    id: String,
    name: String,
    key: String,
    #[serde(
        default,
        alias = "addedAt",
        alias = "added_at",
        alias = "added_at_unix_ms",
        alias = "addedAtUnixMs"
    )]
    added_at: f64,
}

impl PairingStore {
    pub fn authorization(
        &self,
        id: &str,
    ) -> Result<(PairingTrust, Option<GuestPermissions>), SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let records = self.read_grants()?;
        let Some((trust, permissions)) = records.get(id) else {
            return Ok((PairingTrust::Owner, None));
        };
        Ok((
            PairingTrust::decode(trust)?,
            permissions.map(|p| GuestPermissions {
                keyboard: p[0],
                mouse: p[1],
                gamepad: p[2],
                clipboard: p[3],
                audio: p[4],
            }),
        ))
    }

    fn read_grants(&self) -> Result<HostGrants, SessionError> {
        match fs::read(self.path.with_extension("grants.json")) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| SessionError::Store(e.to_string()))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Default::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_authorization(
        &self,
        id: &str,
        trust: PairingTrust,
        permissions: Option<GuestPermissions>,
    ) -> Result<(), SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut records = self.read_grants()?;
        records.insert(
            id.to_owned(),
            (
                trust.encode()?,
                permissions.map(|p| [p.keyboard, p.mouse, p.gamepad, p.clipboard, p.audio]),
            ),
        );
        self.write_grants_unlocked(&records)
    }

    fn write_grants_unlocked(&self, records: &HostGrants) -> Result<(), SessionError> {
        let path = self.path.with_extension("grants.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        use std::io::Write;
        let mut file = options.open(&temporary)?;
        file.write_all(
            &serde_json::to_vec(records).map_err(|e| SessionError::Store(e.to_string()))?,
        )?;
        file.sync_all()?;
        fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn default_path() -> Result<PathBuf, SessionError> {
        let directory = dirs::data_dir()
            .ok_or_else(|| SessionError::Store("Application Support is unavailable".into()))?
            .join("MahoRD");
        Ok(directory.join("host-authorizations.json"))
    }

    pub fn host_default() -> Result<Self, SessionError> {
        Ok(Self::new(Self::default_path()?))
    }

    pub fn service_default_path(program_data: &str) -> PathBuf {
        crate::windows_session::service_store_path(program_data)
    }

    #[cfg(target_os = "windows")]
    pub fn service_default() -> Self {
        let program_data =
            std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
        Self::new(Self::service_default_path(&program_data))
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_all(&self) -> Result<Vec<PairingRecord>, SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        self.load_all_unlocked()
    }

    pub fn load(&self, id: &str) -> Result<Option<PairingRecord>, SessionError> {
        Ok(self.load_all()?.into_iter().find(|record| record.id == id))
    }

    pub fn save(&self, record: PairingRecord) -> Result<(), SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut records = self.load_all_unlocked()?;
        records.retain(|existing| existing.id != record.id);
        records.push(record);
        self.write_unlocked(&records)
    }

    pub fn revoke(&self, id: &str) -> Result<bool, SessionError> {
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut records = self.load_all_unlocked()?;
        let original_len = records.len();
        records.retain(|record| record.id != id);
        let removed = records.len() != original_len;
        if removed {
            self.write_unlocked(&records)?;
        }
        // Drop the authorization alongside the key so no stale grant outlives
        // the pairing (and a leftover grant without a key is cleaned up too).
        let mut grants = self.read_grants()?;
        if grants.remove(id).is_some() {
            self.write_grants_unlocked(&grants)?;
        }
        Ok(removed)
    }

    fn load_all_unlocked(&self) -> Result<Vec<PairingRecord>, SessionError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(SessionError::Io(error)),
        };
        let disk: Vec<DiskPairingRecord> = serde_json::from_slice(&bytes)
            .map_err(|error| SessionError::Store(error.to_string()))?;
        disk.into_iter()
            .map(|record| {
                let key = base64::decode_block(&record.key)
                    .map_err(|error| SessionError::Store(error.to_string()))?;
                let key: [u8; 32] = key
                    .try_into()
                    .map_err(|_| SessionError::Store("pairing key is not 32 bytes".into()))?;
                let unix_seconds = record.added_at + SWIFT_REFERENCE_DATE_OFFSET;
                Ok(PairingRecord {
                    id: record.id,
                    name: record.name,
                    key,
                    added_at_unix_ms: (unix_seconds.max(0.0) * 1_000.0).round() as u64,
                })
            })
            .collect()
    }

    fn write_unlocked(&self, records: &[PairingRecord]) -> Result<(), SessionError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let disk = records
            .iter()
            .map(|record| DiskPairingRecord {
                id: record.id.clone(),
                name: record.name.clone(),
                key: base64::encode_block(&record.key),
                added_at: record.added_at_unix_ms as f64 / 1_000.0 - SWIFT_REFERENCE_DATE_OFFSET,
            })
            .collect::<Vec<_>>();
        let bytes =
            serde_json::to_vec(&disk).map_err(|error| SessionError::Store(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&temporary, &self.path)?;
        #[cfg(unix)]
        {
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

pub fn random_pin() -> String {
    let mut rng = rand::rng();
    let mut bytes = [0_u8; 8];
    rng.fill_bytes(&mut bytes);
    let value = u64::from_le_bytes(bytes) % 100_000_000;
    format!("{value:08}")
}

type HostGrants = std::collections::BTreeMap<String, (Vec<u8>, Option<[bool; 5]>)>;

pub struct ConsentPrompt {
    pub client_name: String,
    response: SyncSender<Option<PairingTrust>>,
}

impl ConsentPrompt {
    pub fn approve(self) {
        self.approve_as(PairingTrust::Owner);
    }

    pub fn approve_as(self, trust: PairingTrust) {
        let _ = self.response.try_send(Some(trust));
    }

    pub fn reject(self) {
        let _ = self.response.try_send(None);
    }

    pub fn respond(self, approved: bool) {
        if approved {
            self.approve_as(PairingTrust::Owner);
        } else {
            self.reject();
        }
    }
}

#[derive(Clone)]
pub struct HostConfig {
    pub tcp_addr: SocketAddr,
    pub udp_addr: SocketAddr,
    pub bootstrap_pin: Option<String>,
    pub pairing_window: Duration,
    pub pairing_store: PairingStore,
    pub host_name: String,
    pub display: DisplayInfo,
    pub frames_per_second: u32,
    pub bitrate: u32,
    pub capture_audio: bool,
    pub consent_sender: Option<mpsc::Sender<ConsentPrompt>>,
    /// Linux only: which output to capture. `None` picks the first output,
    /// which on Hyprland is frequently a headless output that never commits.
    pub output_name: Option<String>,
    pub allow_input: Arc<AtomicBool>,
    pub allow_clipboard: Arc<AtomicBool>,
}

impl HostConfig {
    pub fn host_policy(&self) -> HostPolicy {
        HostPolicy {
            allow_input: self.allow_input.load(Ordering::SeqCst),
            allow_clipboard: self.allow_clipboard.load(Ordering::SeqCst),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn macos_default(
        bootstrap_pin: Option<String>,
        pairing_store: PairingStore,
    ) -> Result<Self, SessionError> {
        let display = ScreenCapture::display_info()?;
        Ok(Self {
            tcp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_TCP_PORT)),
            udp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_UDP_PORT)),
            bootstrap_pin,
            pairing_window: PAIRING_WINDOW,
            pairing_store,
            host_name: hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            display,
            frames_per_second: 60,
            bitrate: 100_000_000,
            capture_audio: true,
            output_name: None,
            consent_sender: None,
            allow_input: Arc::new(AtomicBool::new(true)),
            allow_clipboard: Arc::new(AtomicBool::new(true)),
        })
    }

    #[cfg(target_os = "windows")]
    pub fn windows_default(
        bootstrap_pin: Option<String>,
        pairing_store: PairingStore,
    ) -> Result<Self, SessionError> {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::UI::HiDpi::{
                SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            };
            unsafe {
                let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            }
        }
        let meta = crate::capture_windows::WindowsCapture::primary_output_metadata()
            .map_err(|error| SessionError::Io(io::Error::other(error.to_string())))?;
        Ok(Self {
            tcp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_TCP_PORT)),
            udp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_UDP_PORT)),
            bootstrap_pin,
            pairing_window: PAIRING_WINDOW,
            pairing_store,
            host_name: hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            display: DisplayInfo {
                desktop_x: meta.desktop_x,
                desktop_y: meta.desktop_y,
                logical_width: meta.logical_width,
                logical_height: meta.logical_height,
                pixel_width: meta.pixel_width,
                pixel_height: meta.pixel_height,
                scale_factor_milli: meta.scale_factor_milli,
            },
            frames_per_second: 60,
            bitrate: WINDOWS_DEFAULT_BITRATE,
            capture_audio: false,
            output_name: None,
            consent_sender: None,
            allow_input: Arc::new(AtomicBool::new(true)),
            allow_clipboard: Arc::new(AtomicBool::new(true)),
        })
    }

    #[cfg(target_os = "linux")]
    pub fn linux_default(
        bootstrap_pin: Option<String>,
        pairing_store: PairingStore,
        output_name: Option<String>,
    ) -> Result<Self, SessionError> {
        // A pinned output the compositor no longer advertises must not stop the daemon
        // from starting. The dimension probe follows the same fallback the per-session
        // media path uses, while the configured pin is preserved so the pinned output is
        // preferred again as soon as it returns.
        let probe_output =
            reprobe_output_if_missing(output_name.as_deref(), probe_hyprland_monitors().as_ref());
        let config = LinuxCaptureConfig {
            output_name: probe_output,
            ..LinuxCaptureConfig::default()
        };
        let output = LinuxCapture::probe_output(config)
            .map_err(|error| SessionError::Io(io::Error::other(error.to_string())))?;
        let display = DisplayInfo {
            desktop_x: 0,
            desktop_y: 0,
            logical_width: output.pixel_width,
            logical_height: output.pixel_height,
            pixel_width: output.pixel_width,
            pixel_height: output.pixel_height,
            scale_factor_milli: (output.scale.max(1) as u32) * 1_000,
        };
        Ok(Self {
            tcp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_TCP_PORT)),
            udp_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_UDP_PORT)),
            bootstrap_pin,
            pairing_window: PAIRING_WINDOW,
            pairing_store,
            host_name: hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            display,
            frames_per_second: 60,
            bitrate: LINUX_DEFAULT_BITRATE,
            capture_audio: false,
            output_name,
            consent_sender: None,
            allow_input: Arc::new(AtomicBool::new(true)),
            allow_clipboard: Arc::new(AtomicBool::new(true)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    PreAuth,
    PairingGranted,
    Authenticated,
    Closed,
}

impl SessionState {
    pub fn allows(self, packet_type: PacketType) -> bool {
        match self {
            Self::PreAuth => matches!(
                packet_type,
                PacketType::PairingRequest | PacketType::Handshake
            ),
            Self::PairingGranted => matches!(packet_type, PacketType::Handshake),
            Self::Authenticated => {
                matches!(
                    packet_type,
                    PacketType::InputEvent | PacketType::InputBatch | PacketType::Control
                )
            }
            Self::Closed => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("TLS-PSK failed: {0}")]
    Tls(#[from] TlsPskError),
    #[error("protocol codec failed: {0}")]
    Codec(#[from] ferryx_rd_proto::CodecError),
    #[error("UDP cipher failed: {0}")]
    Cipher(#[from] ferryx_rd_net::DatagramError),
    #[cfg(target_os = "macos")]
    #[error("capture failed: {0}")]
    Capture(#[from] crate::capture_macos::CaptureError),
    #[cfg(target_os = "macos")]
    #[error("encoder failed: {0}")]
    Encode(#[from] crate::encode_vt::EncodeError),
    #[error("pairing store failed: {0}")]
    Store(String),
    #[error("invalid bootstrap PIN")]
    InvalidPin,
    #[error("pairing consent is unavailable")]
    ConsentUnavailable,
    #[error("pairing consent timed out")]
    ConsentTimeout,
    #[error("peer is not authenticated")]
    PreAuth,
    #[error("session is already authenticated")]
    AlreadyAuthenticated,
    #[error("handshake pairing ID does not match the TLS identity")]
    IdentityMismatch,
    #[error("unknown pairing ID")]
    UnknownPairing,
    #[error("session salt is missing")]
    MissingSessionSalt,
    #[error("UDP peer is unavailable")]
    UdpPeerUnavailable,
    #[error("discovery advertisement failed: {0}")]
    Discovery(#[from] ferryx_rd_net::discovery::DiscoveryError),
    #[error("media pipeline stopped")]
    MediaStopped,
    #[error("peer lacks authenticated UDP registration capability")]
    MissingAuthenticatedRegistration,
}

#[derive(Debug)]
enum MediaEvent {
    Video(VideoFrame),
    /// Windows pipeline has no audio source yet; the variant is kept so the
    /// wire shape stays identical across platforms.
    #[allow(dead_code)]
    Audio(Vec<u8>),
    #[cfg(target_os = "windows")]
    TimedAudio {
        pcm: Vec<u8>,
        captured_at: Instant,
    },
    #[cfg(target_os = "macos")]
    MacAudio(Arc<MacAudioSlot>),
    #[cfg(target_os = "macos")]
    MacReady(Arc<crate::capture_macos::Wakeup>),
    #[cfg(target_os = "linux")]
    NativeAudio(Arc<native_pipeline::LatestAudio>),
    #[allow(dead_code)]
    Cursor(CursorUpdate, Option<u32>, Option<CursorShape>),
    Error(String),
}

/// Platform-neutral encoded frame handed from a [`MediaSource`] to the wire.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
    pub capture_at: Instant,
    pub encode_started_at: Instant,
    pub encode_completed_at: Instant,
}

/// Platform-neutral display geometry shared by every media backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayInfo {
    pub desktop_x: i32,
    pub desktop_y: i32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale_factor_milli: u32,
}

impl DisplayInfo {
    pub fn scale_factor(self) -> f32 {
        self.scale_factor_milli as f32 / 1_000.0
    }
}

#[cfg(target_os = "windows")]
const WINDOWS_DEFAULT_BITRATE: u32 = 100_000_000;

#[cfg(target_os = "linux")]
const LINUX_DEFAULT_BITRATE: u32 = 100_000_000;

/// Initial quality budget in bits/pixel/frame, never exceeding the user cap.
fn starting_bitrate(
    width: u32,
    height: u32,
    fps: u32,
    codec: StreamCodecPreference,
    cap: u32,
) -> u32 {
    let divisor = if codec == StreamCodecPreference::H264 {
        6
    } else {
        10
    };
    let estimate = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(u64::from(fps))
        / divisor;
    estimate
        .clamp(1, u64::from(u32::MAX))
        .min(u64::from(cap.max(1))) as u32
}

/// Selects the focused output name from compositor monitor information,
/// falling back to the first available named monitor if none is marked focused.
pub fn select_focused_output(monitors: &serde_json::Value) -> Option<String> {
    let array = monitors.as_array()?;
    if let Some(name) = array
        .iter()
        .find(|monitor| monitor.get("focused").and_then(serde_json::Value::as_bool) == Some(true))
        .and_then(|monitor| monitor.get("name"))
        .and_then(serde_json::Value::as_str)
    {
        return Some(name.to_owned());
    }
    array
        .iter()
        .find_map(|monitor| monitor.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
}

/// Resolves the output target based on strict precedence:
/// 1. Explicit CLI argument (`--output <NAME>`) — strict, preserved as-is.
/// 2. Explicit environment variable (`MAHO_OUTPUT=<NAME>`) — strict, preserved as-is.
/// 3. Auto-detection via compositor monitor information:
///    a. If a monitor has `"focused": true`, select it.
///    b. If no monitor is focused, fall back to the first available named monitor.
///    c. If no monitors are available, return `None`.
pub fn resolve_output_target(
    cli_output: Option<String>,
    env_output: Option<String>,
    monitors: Option<&serde_json::Value>,
) -> Option<String> {
    if let Some(name) = cli_output.filter(|s| !s.trim().is_empty()) {
        return Some(name);
    }
    if let Some(name) = env_output.filter(|s| !s.trim().is_empty()) {
        return Some(name);
    }
    let monitors = monitors?;
    select_focused_output(monitors)
}

/// Returns true if the output name represents a virtual/headless output.
///
/// Compositors like Hyprland create virtual outputs named `HEADLESS-N` (e.g.,
/// `HEADLESS-1`). If a physical monitor drops transiently, we must avoid
/// falling back to a virtual output that displays a blank virtual screen when a
/// real non-virtual monitor is available.
#[cfg(any(target_os = "linux", test))]
pub fn output_is_virtual(name: &str) -> bool {
    name.starts_with("HEADLESS")
}

/// Fallback output selector when a pinned output is missing.
///
/// Priority:
/// 1. Focused monitor, if not virtual;
/// 2. Otherwise the first non-virtual monitor;
/// 3. Otherwise the focused monitor;
/// 4. Otherwise the first monitor;
/// 5. Otherwise None.
#[cfg(any(target_os = "linux", test))]
fn select_fallback_output(monitors: &serde_json::Value) -> Option<String> {
    let array = monitors.as_array()?;

    // 1. Focused monitor, if not virtual
    let focused = array
        .iter()
        .find(|m| m.get("focused").and_then(serde_json::Value::as_bool) == Some(true))
        .and_then(|m| m.get("name"))
        .and_then(serde_json::Value::as_str);

    if let Some(name) = focused {
        if !output_is_virtual(name) {
            return Some(name.to_owned());
        }
    }

    // 2. First non-virtual monitor
    if let Some(name) = array
        .iter()
        .filter_map(|m| m.get("name").and_then(serde_json::Value::as_str))
        .find(|name| !output_is_virtual(name))
    {
        return Some(name.to_owned());
    }

    // 3. Focused monitor (even if virtual)
    if let Some(name) = focused {
        return Some(name.to_owned());
    }

    // 4. First monitor (even if virtual)
    if let Some(name) = array
        .iter()
        .find_map(|m| m.get("name").and_then(serde_json::Value::as_str))
    {
        return Some(name.to_owned());
    }

    // 5. None
    None
}

/// Re-probes the effective output name when a pinned output is missing.
///
/// A pinned output that the compositor temporarily stops advertising (monitor
/// unplugged or renamed) must not kill every later session until a manual
/// restart.
#[cfg(any(target_os = "linux", test))]
pub fn reprobe_output_if_missing(
    pinned: Option<&str>,
    monitors: Option<&serde_json::Value>,
) -> Option<String> {
    let monitors = match monitors {
        Some(m) => m,
        None => return pinned.map(str::to_owned),
    };
    if let Some(name) = pinned {
        let array = monitors.as_array();
        let present = array.is_some_and(|list| {
            list.iter()
                .any(|m| m.get("name").and_then(serde_json::Value::as_str) == Some(name))
        });
        if present {
            return Some(name.to_owned());
        }
        let fallback = select_fallback_output(monitors);
        tracing::warn!(
            pinned = name,
            fallback = fallback.as_deref(),
            "pinned output {name} not found in compositor monitors; falling back to {fallback:?}"
        );
        fallback
    } else {
        select_focused_output(monitors)
    }
}

#[cfg(target_os = "linux")]
fn reprobe_output_name(pinned: Option<&str>) -> Option<String> {
    let monitors = probe_hyprland_monitors();
    reprobe_output_if_missing(pinned, monitors.as_ref())
}

/// Lists live-looking Hyprland instance signatures, newest first.
///
/// Deliberately ignores `HYPRLAND_INSTANCE_SIGNATURE`: that variable is the one
/// under suspicion here, because a daemon started before a Hyprland restart
/// keeps the dead signature for its whole lifetime. Instance directories and
/// their sockets outlive the compositor that created them, so ordering by
/// modification time is what puts the live instance first; the caller still
/// probes each candidate, since only a successful probe proves liveness.
#[cfg(target_os = "linux")]
fn hyprland_instance_candidates() -> Vec<String> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let uid = unsafe { libc::getuid() };
            std::path::PathBuf::from(format!("/run/user/{uid}"))
        });
    let hypr_dir = runtime_dir.join("hypr");
    let Ok(entries) = std::fs::read_dir(hypr_dir) else {
        return Vec::new();
    };
    let mut candidates: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() || !path.join(".socket.sock").exists() {
                return None;
            }
            let name = path.file_name().and_then(|name| name.to_str())?.to_owned();
            let modified = entry.metadata().ok().and_then(|meta| meta.modified().ok());
            Some((modified.unwrap_or(std::time::UNIX_EPOCH), name))
        })
        .collect();
    candidates.sort_by_key(|left| std::cmp::Reverse(left.0));
    candidates.into_iter().map(|(_, name)| name).collect()
}

/// Runs `hyprctl monitors -j` against an explicit instance signature.
///
/// A `None` signature drops any inherited `HYPRLAND_INSTANCE_SIGNATURE` instead
/// of leaving it for `hyprctl` to pick up: the callers only reach the bare probe
/// once the inherited value is known to be empty or already proven stale, so
/// passing it through would just fail the same way a second time.
#[cfg(target_os = "linux")]
fn hyprland_monitors_with_signature(signature: Option<&str>) -> Option<serde_json::Value> {
    let mut cmd = std::process::Command::new("hyprctl");
    match signature {
        Some(signature) => {
            cmd.arg("-i").arg(signature);
        }
        None => {
            cmd.env_remove("HYPRLAND_INSTANCE_SIGNATURE");
        }
    }
    cmd.arg("monitors").arg("-j");
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// Resolves monitor topology, rediscovering the instance when the inherited
/// signature is stale.
///
/// An inherited signature that no longer names a live instance makes `hyprctl`
/// exit non-zero. Treating that as "no compositor" silently degrades input
/// geometry to a single-output fallback sized to the captured output, which
/// then scales every pointer coordinate across the real desktop width and
/// sends the remote cursor to the wrong place.
#[cfg(target_os = "linux")]
fn hyprland_monitors_from_candidates<F>(
    inherited: Option<&str>,
    candidates: &[String],
    mut probe: F,
) -> Option<serde_json::Value>
where
    F: FnMut(Option<&str>) -> Option<serde_json::Value>,
{
    if let Some(signature) = inherited.filter(|signature| !signature.is_empty()) {
        if let Some(monitors) = probe(Some(signature)) {
            return Some(monitors);
        }
        warn!(
            signature,
            "HYPRLAND_INSTANCE_SIGNATURE is stale; rediscovering the live Hyprland instance"
        );
    }
    for signature in candidates {
        if Some(signature.as_str()) == inherited {
            continue;
        }
        if let Some(monitors) = probe(Some(signature)) {
            return Some(monitors);
        }
    }
    probe(None)
}

/// Probes Hyprland monitors using `hyprctl monitors -j`.
#[cfg(target_os = "linux")]
pub fn probe_hyprland_monitors() -> Option<serde_json::Value> {
    let inherited = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    let candidates = hyprland_instance_candidates();
    hyprland_monitors_from_candidates(
        inherited.as_deref(),
        &candidates,
        hyprland_monitors_with_signature,
    )
}

/// Derives `OutputGeometry` from compositor monitor topology.
///
/// Resolves the selected output's logical position and bounds within the full
/// multi-monitor desktop space, accounting for compositor offsets, monitor
/// scaling, and rotation transforms.
#[cfg(target_os = "linux")]
fn output_geometry_from_monitors(
    monitors: &serde_json::Value,
    target_output_name: Option<&str>,
) -> Option<crate::inject_linux::OutputGeometry> {
    let array = monitors.as_array()?;
    if array.is_empty() {
        return None;
    }

    struct MonitorInfo<'a> {
        name: &'a str,
        x: i32,
        y: i32,
        logical_w: u32,
        logical_h: u32,
        focused: bool,
    }

    let mut active_monitors = Vec::new();

    for item in array {
        let disabled = item
            .get("disabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if disabled {
            continue;
        }

        let name = item
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let x = item
            .get("x")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0) as i32;
        let y = item
            .get("y")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0) as i32;
        let width = item
            .get("width")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let height = item
            .get("height")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let scale = item
            .get("scale")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(1.0);
        let transform = item
            .get("transform")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let focused = item
            .get("focused")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if width <= 0.0 || height <= 0.0 {
            continue;
        }

        let (mode_w, mode_h) = match transform {
            1 | 3 | 5 | 7 => (height, width),
            _ => (width, height),
        };
        let effective_scale = if scale > 0.0 { scale } else { 1.0 };
        let logical_w = ((mode_w / effective_scale).round() as u32).max(1);
        let logical_h = ((mode_h / effective_scale).round() as u32).max(1);

        active_monitors.push(MonitorInfo {
            name,
            x,
            y,
            logical_w,
            logical_h,
            focused,
        });
    }

    if active_monitors.is_empty() {
        return None;
    }

    let desktop_x = active_monitors.iter().map(|m| m.x).min().unwrap();
    let desktop_y = active_monitors.iter().map(|m| m.y).min().unwrap();
    let desktop_max_x = active_monitors
        .iter()
        .map(|m| m.x as i64 + m.logical_w as i64)
        .max()
        .unwrap();
    let desktop_max_y = active_monitors
        .iter()
        .map(|m| m.y as i64 + m.logical_h as i64)
        .max()
        .unwrap();
    let desktop_width = (desktop_max_x - desktop_x as i64).max(1) as u32;
    let desktop_height = (desktop_max_y - desktop_y as i64).max(1) as u32;

    let target = match target_output_name {
        Some(name) => active_monitors.iter().find(|m| m.name == name)?,
        None => active_monitors
            .iter()
            .find(|m| m.focused)
            .or_else(|| active_monitors.first())?,
    };

    Some(crate::inject_linux::OutputGeometry {
        x: target.x,
        y: target.y,
        width: target.logical_w,
        height: target.logical_h,
        desktop_x,
        desktop_y,
        desktop_width,
        desktop_height,
    })
}

/// Probes compositor topology to compute the target output geometry and full desktop bounds.
#[cfg(target_os = "linux")]
fn probe_linux_input_geometry(
    target_output_name: Option<&str>,
) -> Option<crate::inject_linux::OutputGeometry> {
    let monitors = probe_hyprland_monitors()?;
    output_geometry_from_monitors(&monitors, target_output_name)
}

/// Name of the compositor's focused output, for hosts that expose several
/// outputs (e.g. Hyprland headless outputs). Best effort: None when the
/// compositor is unknown or the probe fails, which makes the capture fall
/// back to the first output.
#[cfg(target_os = "linux")]
pub fn focused_output_name() -> Option<String> {
    let monitors = probe_hyprland_monitors();
    resolve_output_target(None, std::env::var("MAHO_OUTPUT").ok(), monitors.as_ref())
}

/// Truncates a UTF-8 string to at most `max_bytes` without slicing through a code point.
fn truncate_str_to_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Enumerates host displays for HostDisplayList control messages.
pub fn host_displays(config: &HostConfig) -> Vec<HostDisplay> {
    #[cfg(all(target_os = "macos", not(test)))]
    if let Ok(ids) = core_graphics::display::CGDisplay::active_displays() {
        let displays: Vec<_> = ids
            .into_iter()
            .take(ferryx_rd_proto::MAX_HOST_DISPLAYS)
            .filter_map(|id| {
                ScreenCapture::display_info_for(id)
                    .ok()
                    .map(|info| (id, info))
            })
            .enumerate()
            .map(|(index, (id, info))| HostDisplay {
                index: index as u8,
                width: info.pixel_width,
                height: info.pixel_height,
                primary: id == core_graphics::display::CGDisplay::main().id,
                name: id.to_string(),
            })
            .collect();
        if !displays.is_empty() {
            return displays;
        }
    }
    #[cfg(all(target_os = "windows", not(test)))]
    if let Ok(displays) = crate::capture_windows::enumerate_displays() {
        if !displays.is_empty() {
            return displays;
        }
    }
    #[cfg(all(target_os = "linux", not(test)))]
    {
        if let Some(monitors) = probe_hyprland_monitors() {
            if let Some(array) = monitors.as_array() {
                let mut displays = Vec::new();
                for item in array {
                    if displays.len() >= ferryx_rd_proto::MAX_HOST_DISPLAYS {
                        break;
                    }
                    let disabled = item
                        .get("disabled")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    if disabled {
                        continue;
                    }
                    let raw_name = item
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("output");
                    let name =
                        truncate_str_to_bytes(raw_name, MAX_HOST_DISPLAY_NAME_BYTES).to_string();
                    let width = item
                        .get("width")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0)
                        .max(0.0) as u32;
                    let height = item
                        .get("height")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0)
                        .max(0.0) as u32;
                    let primary = item
                        .get("focused")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let index = displays.len() as u8;
                    displays.push(HostDisplay {
                        index,
                        width,
                        height,
                        primary,
                        name,
                    });
                }
                if !displays.is_empty() {
                    return displays;
                }
            }
        }
    }

    let raw_name = if config.host_name.is_empty() {
        "primary"
    } else {
        &config.host_name
    };
    let name = truncate_str_to_bytes(raw_name, MAX_HOST_DISPLAY_NAME_BYTES).to_string();
    vec![HostDisplay {
        index: 0,
        width: config.display.pixel_width,
        height: config.display.pixel_height,
        primary: true,
        name,
    }]
}

/// Validates requested stream options against bounds and available displays.
pub fn validate_stream_options(
    request: &ferryx_rd_proto::StreamOptionsRequest,
    displays: &[ferryx_rd_proto::HostDisplay],
) -> Result<(), (ferryx_rd_proto::StreamConfigurationErrorCode, String)> {
    if !(100..=1000).contains(&request.scale_permille) {
        return Err((
            ferryx_rd_proto::StreamConfigurationErrorCode::UnsupportedDimensions,
            format!(
                "unsupported scale {} permille; must be 100..=1000",
                request.scale_permille
            ),
        ));
    }
    if !(1..=240).contains(&request.fps) {
        return Err((
            ferryx_rd_proto::StreamConfigurationErrorCode::UnsupportedFps,
            format!("unsupported fps {}; must be 1..=240", request.fps),
        ));
    }
    if (request.display_index as usize) >= displays.len() {
        return Err((
            ferryx_rd_proto::StreamConfigurationErrorCode::InvalidRequest,
            format!(
                "display index {} out of range; {} displays available",
                request.display_index,
                displays.len()
            ),
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const SESSION_MEDIA_QUEUE_DEPTH: usize = 2;
#[cfg(not(target_os = "macos"))]
const SESSION_MEDIA_QUEUE_DEPTH: usize = 16;

trait MediaHandle: Send {
    #[cfg(target_os = "macos")]
    fn readiness(&self) -> Option<Result<(), String>> {
        Some(Ok(()))
    }
    #[cfg(target_os = "linux")]
    fn take_audio_error(&self) -> Option<String> {
        None
    }
    fn codec(&self) -> StreamCodecPreference {
        StreamCodecPreference::H264
    }
    fn force_key_frame(&self) -> Result<(), SessionError>;
    fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError>;
    fn stop(&mut self);
}

/// Supervisor-visible handle to a session's media pipeline.
///
/// A session thread can block in a syscall that never returns, in which case
/// its stack never unwinds and nothing it owns is ever dropped. The capture and
/// encode workers would then keep running at full cost with no client attached.
/// Publishing the pipeline here lets the watchdog stop it from outside, which
/// is the only cleanup available once the owning thread is wedged.
type PipelineSlot = Arc<Mutex<Option<Box<dyn MediaHandle>>>>;

/// Stops a session's media pipeline from outside its own thread.
///
/// Returns true when a pipeline was still registered and has been stopped. The
/// handle is taken out of the slot before `stop()` runs, so the join it performs
/// never happens while the slot is locked.
fn stop_pipeline(slot: &PipelineSlot) -> bool {
    let handle = slot.lock().ok().and_then(|mut slot| slot.take());
    match handle {
        Some(mut handle) => {
            warn!("stopping a media pipeline whose session thread did not unwind");
            handle.stop();
            true
        }
        None => false,
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct MacRecoveryBudget {
    peer_capabilities: Capabilities,
    attempts: u8,
    healthy_since: Option<Instant>,
}

#[cfg(target_os = "macos")]
impl MacRecoveryBudget {
    const HEALTHY_INTERVAL: Duration = Duration::from_secs(30);

    fn ready(&mut self, now: Instant) {
        if self.attempts > 0 {
            self.healthy_since = Some(now);
        }
    }

    fn observe_health(&mut self, now: Instant, media_failed: bool) {
        if media_failed {
            self.healthy_since = None;
        } else if self
            .healthy_since
            .is_some_and(|since| now.saturating_duration_since(since) >= Self::HEALTHY_INTERVAL)
        {
            self.attempts = 0;
            self.healthy_since = None;
        }
    }

    fn start(
        &mut self,
        source: &dyn MediaSource,
        sender: SyncSender<MediaEvent>,
    ) -> Result<Box<dyn MediaHandle>, SessionError> {
        if self.attempts >= 2 {
            return Err(SessionError::Io(io::Error::other(
                "macOS capture recovery exhausted after two consecutive attempts",
            )));
        }
        self.attempts += 1;
        self.healthy_since = None;
        source.start_with_capabilities(sender, self.peer_capabilities)
    }
}

#[cfg(target_os = "macos")]
struct PendingMacPipeline {
    handle: Box<dyn MediaHandle>,
    receiver: Receiver<MediaEvent>,
    restart: StreamRestart,
    response: Option<StreamOptionsActive>,
}

trait MediaSource: Send + Sync {
    fn start_with_capabilities(
        &self,
        sender: SyncSender<MediaEvent>,
        peer: Capabilities,
    ) -> Result<Box<dyn MediaHandle>, SessionError>;
    #[cfg(target_os = "windows")]
    fn supports_cpu_scaling(&self) -> bool {
        true
    }
    #[cfg(target_os = "windows")]
    fn output_target(&self) -> Option<TargetDisplay> {
        None
    }
    #[cfg(test)]
    fn sender_exited(&self) {}
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
struct MacAudioSlot {
    packets: Mutex<std::collections::VecDeque<(Instant, Vec<u8>)>>,
    skipped: std::sync::atomic::AtomicU32,
}

#[cfg(target_os = "macos")]
impl MacAudioSlot {
    const DEADLINE: Duration = Duration::from_millis(100);
    const CAPACITY: usize = 5;

    fn publish(&self, captured_at: Instant, bytes: Vec<u8>, now: Instant) {
        if now.saturating_duration_since(captured_at) > Self::DEADLINE {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut packets = self.packets.lock().expect("audio queue poisoned");
        while packets
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > Self::DEADLINE)
            || packets.len() >= Self::CAPACITY
        {
            packets.pop_front();
            self.skipped.fetch_add(1, Ordering::Relaxed);
        }
        packets.push_back((captured_at, bytes));
    }

    fn take(&self, now: Instant) -> Option<Vec<u8>> {
        self.take_timed(now).map(|(_, bytes)| bytes)
    }

    fn take_timed(&self, now: Instant) -> Option<(Instant, Vec<u8>)> {
        let mut packets = self.packets.lock().expect("audio queue poisoned");
        while let Some((captured_at, bytes)) = packets.pop_front() {
            if now.saturating_duration_since(captured_at) <= Self::DEADLINE {
                return Some((captured_at, bytes));
            }
            self.skipped.fetch_add(1, Ordering::Relaxed);
        }
        None
    }
}

#[cfg(target_os = "macos")]
struct MacMediaSource {
    config: CaptureConfig,
    encoder: EncoderConfig,
    codec: VtCodec,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct MacMediaControls {
    force_key_frame: bool,
    bitrate: Option<u32>,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct MacMediaState {
    ready: Mutex<Option<Result<(), String>>>,
    cancelled: std::sync::atomic::AtomicBool,
    controls: Mutex<MacMediaControls>,
    encoder: Mutex<Option<Arc<VideoToolboxEncoder>>>,
    wakeup: Mutex<Option<Arc<crate::capture_macos::Wakeup>>>,
    terminal: Mutex<Option<Arc<crate::capture_macos::TerminalSignal>>>,
}

#[cfg(target_os = "macos")]
impl MacMediaState {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }

    // Keep the pending compressed packet until sent. Only cancellation or an
    // output disconnect may abandon it; raw capture has its own bounded queue.
    fn publish(&self, sender: &SyncSender<MediaEvent>, mut event: MediaEvent) -> bool {
        let error_deadline =
            matches!(event, MediaEvent::Error(_)).then(|| Instant::now() + Duration::from_secs(1));
        while !self.is_cancelled() {
            if error_deadline.is_none()
                && self
                    .terminal
                    .lock()
                    .expect("media terminal poisoned")
                    .as_ref()
                    .is_some_and(|terminal| terminal.get().is_some())
            {
                return false;
            }
            if error_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return false;
            }
            match sender.try_send(event) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Full(pending)) => event = pending,
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
            }
            let wakeup = self.wakeup.lock().expect("media wakeup poisoned").clone();
            if let Some(wakeup) = wakeup {
                wakeup.wait(Duration::from_secs(1));
            } else if let Some(deadline) = error_deadline {
                thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
            } else {
                thread::park();
            }
        }
        false
    }

    fn flush_controls(
        &self,
        sender: &SyncSender<crate::encode_vt::Command>,
    ) -> Result<(), SessionError> {
        use crate::encode_vt::Command;
        let mut controls = self
            .controls
            .lock()
            .map_err(|_| SessionError::MediaStopped)?;
        if controls.force_key_frame {
            match sender.try_send(Command::ForceKeyFrame) {
                Ok(()) => controls.force_key_frame = false,
                Err(mpsc::TrySendError::Full(_)) => return Ok(()),
                Err(mpsc::TrySendError::Disconnected(_)) => return Err(SessionError::MediaStopped),
            }
        }
        if let Some(bitrate) = controls.bitrate {
            match sender.try_send(Command::UpdateBitrate(bitrate)) {
                Ok(()) => controls.bitrate = None,
                Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => return Err(SessionError::MediaStopped),
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
struct MacMediaHandle {
    state: Arc<MacMediaState>,
    worker: Option<thread::JoinHandle<()>>,
}

// Created and destroyed on the owner thread, including encoder init failures.
#[cfg(target_os = "macos")]
struct MacCaptureOwner(Option<ScreenCapture>);

#[cfg(target_os = "macos")]
impl Drop for MacCaptureOwner {
    fn drop(&mut self) {
        if let Some(capture) = self.0.take() {
            if let Err(error) = capture.stop() {
                warn!(%error, "macOS capture stop failed");
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl MacMediaHandle {
    fn spawn(
        sender: SyncSender<MediaEvent>,
        run: impl FnOnce(&Arc<MacMediaState>, &SyncSender<MediaEvent>) -> Result<(), SessionError>
            + Send
            + 'static,
    ) -> Result<Self, SessionError> {
        let state = Arc::new(MacMediaState::default());
        let worker_state = Arc::clone(&state);
        let worker = thread::Builder::new()
            .name("ferryx-rd-host-macos-media".into())
            .spawn(move || {
                if let Err(error) = run(&worker_state, &sender) {
                    *worker_state.ready.lock().expect("media readiness poisoned") =
                        Some(Err(error.to_string()));
                    if !worker_state.is_cancelled() {
                        worker_state.publish(&sender, MediaEvent::Error(error.to_string()));
                    }
                }
                worker_state
                    .cancelled
                    .store(true, std::sync::atomic::Ordering::Release);
            })?;
        Ok(Self {
            state,
            worker: Some(worker),
        })
    }

    fn wake(&self) {
        if let Some(wakeup) = self
            .state
            .wakeup
            .lock()
            .expect("media wakeup poisoned")
            .as_ref()
        {
            wakeup.notify();
        }
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }
}

#[cfg(target_os = "macos")]
impl MediaSource for MacMediaSource {
    fn start_with_capabilities(
        &self,
        sender: SyncSender<MediaEvent>,
        peer: Capabilities,
    ) -> Result<Box<dyn MediaHandle>, SessionError> {
        let independent = peer.contains(Capabilities::CURSOR_SHAPE);
        let config = self.config;
        let encoder_config = self.encoder;
        let codec = self.codec;
        let handle = MacMediaHandle::spawn(sender, move |state, sender| {
            // SCK objects never cross this thread boundary. The caller already
            // owns the cancellation handle while native callbacks are pending.
            let (capture, capture_rx) = ScreenCapture::start_surface_with_cursor_cancellable(
                config,
                !independent,
                &state.cancelled,
                Instant::now() + Duration::from_secs(15),
            )?;
            let _capture = MacCaptureOwner(Some(capture));
            let mut cursor_worker = native_pipeline::Workers::<()>::new();
            if independent {
                let display = ScreenCapture::display_info_for(
                    config
                        .display_id
                        .unwrap_or_else(|| core_graphics::display::CGDisplay::main().id),
                )?;
                let cursor_sender = sender.clone();
                cursor_worker.spawn("maho-macos-cursor", false, move |handoff| {
                    let mut cache = crate::cursor::ShapeCache::default();
                    let mut previous = None;
                    let mut pending_shape = None;
                    while !handoff.is_stopped() {
                        let started = Instant::now();
                        let shape_id =
                            if let Some((id, changed)) = crate::cursor::mac_shape(&mut cache) {
                                if changed.is_some() {
                                    pending_shape = changed;
                                }
                                Some(id)
                            } else {
                                pending_shape = None;
                                cache = crate::cursor::ShapeCache::default();
                                None
                            };
                        if let Ok((x, y)) = ScreenCapture::cursor_position(display) {
                            let current = (x, y, shape_id);
                            if previous != Some(current) || pending_shape.is_some() {
                                let update = CursorUpdate {
                                    x,
                                    y,
                                    cursor_type: u8::from(x >= 0.0 && y >= 0.0),
                                };
                                match cursor_sender.try_send(MediaEvent::Cursor(
                                    update,
                                    shape_id,
                                    pending_shape.take(),
                                )) {
                                    Ok(()) => previous = Some(current),
                                    Err(mpsc::TrySendError::Full(MediaEvent::Cursor(
                                        _,
                                        _,
                                        shape,
                                    ))) => pending_shape = shape,
                                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                                    Err(_) => unreachable!(),
                                }
                            }
                        }
                        handoff.pace_until(started + Duration::from_nanos(1_000_000_000 / 240));
                    }
                })?;
                cursor_worker.handoff.activate();
            }
            if state.is_cancelled() {
                return Ok(());
            }
            let (encoder, encoded_rx) = VideoToolboxEncoder::start_hardware_fallback(
                encoder_config,
                EncoderOptions {
                    codec,
                    input: InputKind::Nv12Surface,
                    require_hardware: true,
                    wakeup: Some(Arc::clone(&capture_rx.wakeup)),
                },
            )?;
            info!(codec = ?encoder.info().codec, hardware = encoder.is_hardware(), "macOS encoder initialized");
            *state.terminal.lock().expect("media terminal poisoned") =
                Some(Arc::clone(&capture_rx.terminal));
            *state.wakeup.lock().expect("media wakeup poisoned") =
                Some(Arc::clone(&capture_rx.wakeup));
            let encoder = Arc::new(encoder);
            *state.encoder.lock().expect("media encoder poisoned") = Some(Arc::clone(&encoder));
            if !state.publish(sender, MediaEvent::MacReady(Arc::clone(&capture_rx.wakeup))) {
                state.encoder.lock().expect("media encoder poisoned").take();
                return Ok(());
            }
            let audio_allowed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let audio_ready = Arc::clone(&audio_allowed);
            let audio_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let audio_stopped = Arc::clone(&audio_stop);
            let audio_state = Arc::clone(state);
            let audio_sender = sender.clone();
            let audio = thread::Builder::new()
                .name("ferryx-rd-host-macos-audio".into())
                .spawn(move || {
                    let slot = Arc::new(MacAudioSlot::default());
                    while !audio_state.is_cancelled()
                        && !audio_stopped.load(std::sync::atomic::Ordering::Acquire)
                    {
                        match capture_rx.audio.recv_timeout(Duration::from_secs(1)) {
                            Ok(CaptureEvent::Audio {
                                pcm_f32_le,
                                captured_at,
                            }) => {
                                if !audio_ready.load(std::sync::atomic::Ordering::Acquire) {
                                    continue;
                                }
                                slot.publish(captured_at, pcm_f32_le, Instant::now());
                                match audio_sender.try_send(MediaEvent::MacAudio(Arc::clone(&slot)))
                                {
                                    Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(mpsc::RecvTimeoutError::Timeout) | Ok(_) => {}
                        }
                    }
                })?;
            let mut awaiting_keyframe = true;
            let deadline = Instant::now() + Duration::from_secs(15);
            let result = (|| {
                while !state.is_cancelled() {
                    if awaiting_keyframe && Instant::now() >= deadline {
                        return Err(SessionError::Io(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "macOS pipeline keyframe deadline expired",
                        )));
                    }
                    if let Some(error) = capture_rx.terminal.get() {
                        return Err(SessionError::Io(io::Error::other(error)));
                    }
                    if let Some(frame) = capture_rx.surface.take() {
                        encoder.submit_surface(frame)?;
                    }
                    state.flush_controls(&encoder.sender)?;
                    let mut progressed = false;
                    // One packet at a time preserves compressed output order and
                    // bounds pending memory even if the session receiver stalls.
                    match encoded_rx.try_recv() {
                        Ok(Ok(frame)) => {
                            progressed = true;
                            if awaiting_keyframe && !frame.is_key_frame {
                                continue;
                            }
                            if awaiting_keyframe {
                                awaiting_keyframe = false;
                                audio_allowed.store(true, std::sync::atomic::Ordering::Release);
                                *state.ready.lock().expect("media readiness poisoned") =
                                    Some(Ok(()));
                            }
                            if let Some(trace) = host_trace::enabled() {
                                trace.record(host_trace::Record {
                                    event: 21,
                                    value: trace.time(frame.callback_at),
                                    ..Default::default()
                                });
                            }
                            if !state.publish(
                                sender,
                                MediaEvent::Video(VideoFrame {
                                    data: frame.data,
                                    is_key_frame: frame.is_key_frame,
                                    capture_at: frame.capture_at,
                                    encode_started_at: frame.encode_started_at,
                                    encode_completed_at: frame.encode_completed_at,
                                }),
                            ) {
                                return Ok(());
                            }
                        }
                        Ok(Err(error)) => return Err(SessionError::from(error)),
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            return Err(SessionError::MediaStopped)
                        }
                    }
                    match capture_rx.video.try_recv() {
                        Ok(CaptureEvent::Video(frame)) => {
                            progressed = true;
                            if !submit_capture_frame(&encoder.sender, frame) {
                                return Err(SessionError::MediaStopped);
                            }
                        }
                        Ok(CaptureEvent::Stopped(error)) => {
                            return Err(SessionError::Io(io::Error::other(error)));
                        }
                        // Audio is routed to its own queue by the capture callback.
                        Ok(CaptureEvent::Audio { .. }) => {}
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            return Err(SessionError::MediaStopped)
                        }
                    }
                    if !progressed {
                        capture_rx.wakeup.wait(Duration::from_secs(1));
                    }
                }
                Ok(())
            })();
            audio_stop.store(true, std::sync::atomic::Ordering::Release);
            if audio.join().is_err() {
                warn!("macOS audio worker panicked");
            }
            // Release native output backpressure before stopping its producer.
            // No bridge spawn can fail after capture/encoder initialization.
            drop(encoded_rx);
            state.encoder.lock().expect("media encoder poisoned").take();
            drop(encoder);
            if let Some(error) = capture_rx.terminal.get() {
                return Err(SessionError::Io(io::Error::other(error)));
            }
            result
        })?;
        Ok(Box::new(handle))
    }
}

#[cfg(target_os = "macos")]
fn submit_capture_frame(
    sender: &SyncSender<crate::encode_vt::Command>,
    frame: CaptureFrame,
) -> bool {
    match sender.try_send(crate::encode_vt::Command::Frame(frame)) {
        Ok(()) | Err(mpsc::TrySendError::Full(_)) => true,
        Err(mpsc::TrySendError::Disconnected(_)) => false,
    }
}

#[cfg(target_os = "macos")]
impl MediaHandle for MacMediaHandle {
    fn codec(&self) -> StreamCodecPreference {
        self.state
            .encoder
            .lock()
            .expect("media encoder poisoned")
            .as_ref()
            .map(|encoder| match encoder.info().codec {
                VtCodec::Hevc => StreamCodecPreference::Hevc,
                VtCodec::H264 => StreamCodecPreference::H264,
            })
            .unwrap_or(StreamCodecPreference::Auto)
    }

    fn readiness(&self) -> Option<Result<(), String>> {
        self.state
            .ready
            .lock()
            .expect("media readiness poisoned")
            .clone()
    }

    fn force_key_frame(&self) -> Result<(), SessionError> {
        if self.state.is_cancelled() {
            return Err(SessionError::MediaStopped);
        }
        if let Some(encoder) = self
            .state
            .encoder
            .lock()
            .map_err(|_| SessionError::MediaStopped)?
            .as_ref()
        {
            encoder.force_key_frame()?;
            return Ok(());
        }
        self.state
            .controls
            .lock()
            .map_err(|_| SessionError::MediaStopped)?
            .force_key_frame = true;
        self.wake();
        Ok(())
    }

    fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError> {
        if self.state.is_cancelled() {
            return Err(SessionError::MediaStopped);
        }
        let encoder = self
            .state
            .encoder
            .lock()
            .map_err(|_| SessionError::MediaStopped)?
            .clone();
        if let Some(encoder) = encoder {
            let applied = encoder.update_bitrate_confirmed(bitrate, Duration::from_secs(2))?;
            if applied != bitrate {
                return Err(SessionError::Io(io::Error::other(format!(
                    "requested bitrate {bitrate}, applied {applied}"
                ))));
            }
            return Ok(());
        }
        self.state
            .controls
            .lock()
            .map_err(|_| SessionError::MediaStopped)?
            .bitrate = Some(bitrate);
        self.wake();
        Ok(())
    }

    fn stop(&mut self) {
        self.state
            .cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        self.wake();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                warn!("macOS media worker panicked");
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacMediaHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod mac_media_tests {
    use super::*;
    use crate::encode_vt::Command;

    #[test]
    fn audio_fifo_preserves_packets_and_deadlines_signal_discontinuity() {
        let slot = MacAudioSlot::default();
        let now = Instant::now();
        slot.publish(now, vec![1; 8], now);
        slot.publish(now, vec![2; 16], now);
        assert_eq!(slot.take(now), Some(vec![1; 8]));
        assert_eq!(slot.take(now), Some(vec![2; 16]));
        assert_eq!(slot.skipped.swap(0, Ordering::Relaxed), 0);
        slot.publish(now, vec![3; 8], now);
        assert_eq!(
            slot.take(now + MacAudioSlot::DEADLINE + Duration::from_nanos(1)),
            None
        );
        assert_eq!(slot.skipped.load(Ordering::Relaxed), 1);
        assert!(slot.take(now).is_none());
    }

    #[test]
    fn readiness_is_nonblocking_and_distinguishes_failure_from_ready() {
        let (output, _receiver) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::channel();
        let (reported, report) = mpsc::channel();
        let mut handle = MacMediaHandle::spawn(output, move |state, _| {
            wait.recv_timeout(BOUND).unwrap();
            *state.ready.lock().unwrap() = Some(Ok(()));
            reported.send(()).unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(handle.readiness(), None);
        release.send(()).unwrap();
        report.recv_timeout(BOUND).unwrap();
        assert_eq!(handle.readiness(), Some(Ok(())));
        handle.stop();
    }

    #[test]
    fn mac_recovery_preserves_negotiated_cursor_capabilities() {
        struct RecordingSource(Mutex<Vec<Capabilities>>);
        impl MediaSource for RecordingSource {
            fn start_with_capabilities(
                &self,
                _: SyncSender<MediaEvent>,
                peer: Capabilities,
            ) -> Result<Box<dyn MediaHandle>, SessionError> {
                self.0.lock().unwrap().push(peer);
                Err(SessionError::MediaStopped)
            }
        }
        let peer = Capabilities::CURSOR_SHAPE | Capabilities::CURSOR_METADATA;
        let source = RecordingSource(Mutex::new(Vec::new()));
        let mut recovery = MacRecoveryBudget {
            peer_capabilities: peer,
            ..Default::default()
        };
        for _ in 0..2 {
            let (sender, _receiver) = mpsc::sync_channel(1);
            assert!(recovery.start(&source, sender).is_err());
        }
        assert_eq!(*source.0.lock().unwrap(), [peer, peer]);
    }

    #[test]
    fn failing_media_source_has_a_finite_recovery_budget() {
        struct FailingSource(std::sync::atomic::AtomicUsize);
        impl MediaSource for FailingSource {
            fn start_with_capabilities(
                &self,
                _: SyncSender<MediaEvent>,
                _peer: Capabilities,
            ) -> Result<Box<dyn MediaHandle>, SessionError> {
                self.0.fetch_add(1, Ordering::Relaxed);
                Err(SessionError::MediaStopped)
            }
        }
        let source = FailingSource(std::sync::atomic::AtomicUsize::new(0));
        let mut recovery = MacRecoveryBudget::default();
        for _ in 0..4 {
            let (sender, _receiver) = mpsc::sync_channel(1);
            assert!(recovery.start(&source, sender).is_err());
        }
        assert_eq!(source.0.load(Ordering::Relaxed), 2);

        let now = Instant::now();
        let mut recovery = MacRecoveryBudget::default();
        let (sender, _receiver) = mpsc::sync_channel(1);
        assert!(recovery.start(&source, sender).is_err());
        recovery.ready(now);
        recovery.observe_health(
            now + MacRecoveryBudget::HEALTHY_INTERVAL - Duration::from_nanos(1),
            false,
        );
        assert_eq!(
            recovery.attempts, 1,
            "readiness alone must not reset attempts"
        );
        recovery.observe_health(now + MacRecoveryBudget::HEALTHY_INTERVAL, false);
        assert_eq!(recovery.attempts, 0);
        for _ in 0..3 {
            let (sender, _receiver) = mpsc::sync_channel(1);
            assert!(recovery.start(&source, sender).is_err());
        }
        assert_eq!(
            source.0.load(Ordering::Relaxed),
            5,
            "healthy recovery permits two new attempts, not three"
        );

        recovery.ready(now);
        recovery.observe_health(now + Duration::from_secs(1), true);
        recovery.observe_health(now + MacRecoveryBudget::HEALTHY_INTERVAL, false);
        assert_eq!(
            recovery.attempts, 2,
            "a failure interrupts the healthy interval"
        );
    }

    const BOUND: Duration = Duration::from_secs(2);

    #[test]
    fn pending_startup_accepts_coalesced_controls_and_stop_joins() {
        // Given: the production owner-worker protocol with a native callback
        // that never arrives. The non-Send resource stays inside that owner.
        let (output, _receiver) = mpsc::sync_channel(1);
        let (entered_tx, entered) = mpsc::channel();
        let (exited_tx, exited) = mpsc::channel();
        let handle = MacMediaHandle::spawn(output, move |state, _| {
            let resource = std::rc::Rc::new(());
            entered_tx.send(()).unwrap();
            while !state.is_cancelled() {
                thread::park();
            }
            drop(resource);
            exited_tx.send(()).unwrap();
            Ok(())
        })
        .unwrap();
        entered.recv_timeout(BOUND).unwrap();
        // When: controls arrive during startup, followed by disconnect.
        for bitrate in 1..=10_000 {
            handle.force_key_frame().unwrap();
            handle.update_bitrate(bitrate).unwrap();
        }
        {
            let controls = handle.state.controls.lock().unwrap();
            assert!(controls.force_key_frame);
            assert_eq!(controls.bitrate, Some(10_000));
        }
        let (done_tx, done) = mpsc::channel();
        let stopper = thread::spawn(move || {
            let mut handle = handle;
            handle.stop();
            assert!(matches!(
                handle.force_key_frame(),
                Err(SessionError::MediaStopped)
            ));
            handle.stop(); // Idempotent, including subsequent Drop.
            done_tx.send(()).unwrap();
        });
        // Then: stop owns the join and does not require a native callback.
        let stopped = done.recv_timeout(BOUND);
        assert_eq!(exited.recv_timeout(BOUND), Ok(()));
        stopper.join().unwrap();
        assert_eq!(stopped, Ok(()));
    }

    #[test]
    fn full_output_stop_joins_before_receiver_is_released() {
        // Given: full output with its receiver intentionally retained.
        let (output, receiver) = mpsc::sync_channel(1);
        output.send(MediaEvent::Audio(vec![1])).unwrap();
        let (entered_tx, entered) = mpsc::channel();
        let handle = MacMediaHandle::spawn(output, move |state, sender| {
            entered_tx.send(()).unwrap();
            state.publish(sender, MediaEvent::Audio(vec![2]));
            Ok(())
        })
        .unwrap();
        entered.recv_timeout(BOUND).unwrap();
        let (done_tx, done) = mpsc::channel();
        // When: disconnect while the worker forwards to the full queue.
        let stopper = thread::spawn(move || {
            let mut handle = handle;
            handle.stop();
            done_tx.send(()).unwrap();
        });
        let stopped = done.recv_timeout(BOUND);
        drop(receiver); // Rescue a blocking-send regression before asserting.
        stopper.join().unwrap();
        // Then: output backpressure did not prevent owned shutdown.
        assert_eq!(stopped, Ok(()), "full output blocked media stop");
    }

    #[test]
    fn startup_controls_survive_full_encoder_queue_and_deliver_latest_bitrate() {
        // Given: startup pending and the actual encoder command queue full.
        let (output, _receiver) = mpsc::sync_channel(1);
        let (commands, receiver) = mpsc::sync_channel(2);
        commands.send(Command::UpdateBitrate(1)).unwrap();
        commands.send(Command::UpdateBitrate(2)).unwrap();
        let (resume_tx, resume) = mpsc::channel();
        let (full_tx, full) = mpsc::channel();
        let (drained_tx, drained) = mpsc::channel();
        let (delivered_tx, delivered) = mpsc::channel();
        let mut handle = MacMediaHandle::spawn(output, move |state, _| {
            resume.recv_timeout(BOUND).unwrap();
            state.flush_controls(&commands)?;
            full_tx.send(()).unwrap();
            drained.recv_timeout(BOUND).unwrap();
            state.flush_controls(&commands)?;
            delivered_tx.send(()).unwrap();
            Ok(())
        })
        .unwrap();
        // When: repeated controls are queued before startup completes.
        for bitrate in 3..=10_000 {
            handle.force_key_frame().unwrap();
            handle.update_bitrate(bitrate).unwrap();
        }
        resume_tx.send(()).unwrap();
        full.recv_timeout(BOUND).unwrap();
        assert!(matches!(
            receiver.recv_timeout(BOUND),
            Ok(Command::UpdateBitrate(1))
        ));
        assert!(matches!(
            receiver.recv_timeout(BOUND),
            Ok(Command::UpdateBitrate(2))
        ));
        drained_tx.send(()).unwrap();
        delivered.recv_timeout(BOUND).unwrap();
        // Then: overload retained exactly one keyframe and the latest bitrate.
        assert!(matches!(
            receiver.recv_timeout(BOUND),
            Ok(Command::ForceKeyFrame)
        ));
        assert!(matches!(
            receiver.recv_timeout(BOUND),
            Ok(Command::UpdateBitrate(10_000))
        ));
        handle.stop();
    }

    #[test]
    fn startup_permission_error_is_forwarded_as_media_error() {
        // Given: the native boundary rejects Screen Recording permission.
        let (output, receiver) = mpsc::sync_channel(1);
        let mut handle = MacMediaHandle::spawn(output, |_, _| {
            Err(crate::capture_macos::CaptureError::PermissionDenied.into())
        })
        .unwrap();
        // When: observe startup through the same session output protocol.
        let event = receiver.recv_timeout(BOUND).unwrap();
        handle.stop();
        // Then: a failure is emitted, not a synthetic desktop frame.
        assert!(matches!(event, MediaEvent::Error(_)));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}

#[cfg(test)]
#[derive(Clone)]
struct SyntheticMediaSource {
    frame_count: u32,
    restart_starts: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(test)]
struct SyntheticMediaHandle(Option<thread::JoinHandle<()>>);

#[cfg(test)]
impl MediaSource for SyntheticMediaSource {
    fn start_with_capabilities(
        &self,
        sender: SyncSender<MediaEvent>,
        _peer: Capabilities,
    ) -> Result<Box<dyn MediaHandle>, SessionError> {
        self.restart_starts.fetch_add(1, Ordering::SeqCst);
        let count = self.frame_count;
        let producer = thread::spawn(move || {
            for index in 0..count {
                let capture_at = Instant::now();
                let encode_started_at = Instant::now();
                let mut data = Vec::new();
                let nalu = [0x26, 0x01, index as u8];
                data.extend_from_slice(&(nalu.len() as u32).to_be_bytes());
                data.extend_from_slice(&nalu);
                let frame = VideoFrame {
                    data,
                    is_key_frame: index == 0,
                    capture_at,
                    encode_started_at,
                    encode_completed_at: Instant::now(),
                };
                if sender.send(MediaEvent::Video(frame)).is_err() {
                    break;
                }
            }
        });
        Ok(Box::new(SyntheticMediaHandle(Some(producer))))
    }
}

#[cfg(test)]
impl MediaHandle for SyntheticMediaHandle {
    fn force_key_frame(&self) -> Result<(), SessionError> {
        Ok(())
    }

    fn update_bitrate(&self, _bitrate: u32) -> Result<(), SessionError> {
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(producer) = self.0.take() {
            producer.join().unwrap();
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos", test))]
#[path = "native_pipeline.rs"]
mod native_pipeline;

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn native_media_error<T>(
    handoff: &native_pipeline::Handoff<T>,
    sender: &SyncSender<MediaEvent>,
    error: String,
) {
    // Never block a failing worker behind compressed output. Cancellation wakes
    // its sibling before reporting; a full queue is logged, not waited upon.
    handoff.stop();
    warn!(%error, "Native media pipeline stopped");
    match sender.try_send(MediaEvent::Error(error)) {
        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
        Err(mpsc::TrySendError::Full(_)) => {
            warn!("Native media error notification could not enter the full media queue");
        }
    }
}

/// `MAHO_PIPELINE=cpu` forces the CPU capture/convert/encode path; anything
/// else (including unset) selects the GPU zero-copy pipeline, which itself
/// falls back to the CPU path when GPU initialization fails.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn gpu_pipeline_requested() -> bool {
    !std::env::var("MAHO_PIPELINE").is_ok_and(|value| value.trim().eq_ignore_ascii_case("cpu"))
}

#[cfg(target_os = "linux")]
fn select_linux_source(cpu: LinuxMediaSource) -> Arc<dyn MediaSource> {
    if gpu_pipeline_requested() {
        media_gpu_linux::gpu_source(cpu)
    } else {
        Arc::new(cpu)
    }
}

#[cfg(target_os = "windows")]
fn select_windows_source(
    cpu: WindowsMediaSource,
    target: Option<(u32, u32)>,
    preference: StreamCodecPreference,
) -> Arc<dyn MediaSource> {
    if gpu_pipeline_requested() {
        let codec = if preference == StreamCodecPreference::H264 {
            VideoCodec::H264
        } else {
            VideoCodec::Hevc
        };
        media_gpu_windows::gpu_source_with_options(cpu, target, codec)
    } else {
        Arc::new(cpu)
    }
}

#[cfg(target_os = "windows")]
#[path = "media_gpu_windows.rs"]
mod media_gpu_windows;

#[cfg(target_os = "linux")]
#[path = "linux_capture_pipeline.rs"]
mod linux_capture_pipeline;
#[cfg(target_os = "linux")]
#[path = "media_gpu_linux.rs"]
mod media_gpu_linux;

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct WindowsMediaSource {
    display_index: usize,
    fps: u32,
    width: u32,
    height: u32,
    bitrate: u32,
    codec: VideoCodec,
    capture_audio: bool,
    /// Captured display origin in virtual-desktop physical pixels. `GetCursorInfo`
    /// reports virtual-desktop coordinates, so this offset must be subtracted
    /// before normalizing against the captured display.
    desktop_x: i32,
    desktop_y: i32,
}

#[cfg(target_os = "windows")]
use crate::windows_logic::freshness::RawFrame as WindowsRawFrame;

#[cfg(target_os = "windows")]
pub(super) mod display_power {
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetThreadExecutionState(es_flags: u32) -> u32;
    }

    /// Keeps the console display (and system) awake while a streaming session
    /// is active — DXGI Desktop Duplication produces no frames while the panel
    /// is in DPMS sleep, which would leave clients with a black canvas.
    pub fn set_keep_awake(enabled: bool) {
        unsafe {
            let flags = if enabled {
                ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
            } else {
                ES_CONTINUOUS
            };
            SetThreadExecutionState(flags);
        }
    }
}

#[cfg(target_os = "windows")]
pub(super) mod cursor_jiggle {
    #[repr(C)]
    #[derive(Default)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    #[link(name = "user32")]
    extern "system" {
        pub fn GetCursorPos(point: *mut Point) -> i32;
        fn SetCursorPos(x: i32, y: i32) -> i32;
    }

    /// Nudges the cursor by one pixel (alternating direction). Pointer updates
    /// force DWM to compose a frame even on an otherwise static desktop, which
    /// unblocks Desktop Duplication before the first frame ever arrives. Only
    /// called while no frame has been captured yet, so active sessions are
    /// never disturbed.
    pub fn nudge(odd: bool) {
        unsafe {
            let mut point = Point::default();
            if GetCursorPos(&mut point) != 0 {
                let dx = if odd { 1 } else { -1 };
                SetCursorPos(point.x + dx, point.y);
            }
        }
    }
}

#[cfg(target_os = "windows")]
struct WindowsSessionEncoder {
    encoder: MediaFoundationEncoder,
    /// `(width << 32) | height` of the newest captured frame, reported by the
    /// capture worker so a desktop resolution change can be adopted instead
    /// of failing every subsequent frame with a size mismatch.
    frame_dims: Arc<AtomicU64>,
}

#[cfg(target_os = "windows")]
impl native_pipeline::Encoder<WindowsRawFrame> for WindowsSessionEncoder {
    type Output = VideoFrame;
    type Error = String;

    fn selected(&mut self, frame: &WindowsRawFrame) {
        if let Some(trace) = host_trace::enabled() {
            trace.record(host_trace::Record {
                event: 19,
                frame: frame.capture_id,
                value: trace.time(frame.published_at),
                repeat: frame.repeat,
                ..Default::default()
            });
        }
    }

    fn force_keyframe(&mut self) {
        self.encoder.force_key_frame();
    }

    fn bitrate(&mut self, bitrate: u32) -> Result<Vec<VideoFrame>, String> {
        self.encoder
            .update_bitrate(bitrate)
            .map_err(|error| format!("mf bitrate: {error}"))?;
        Ok(Vec::new())
    }

    fn encode(&mut self, frame: WindowsRawFrame) -> Result<Vec<VideoFrame>, String> {
        let trace = host_trace::enabled();
        if let Some(trace) = trace {
            let (content, residence) = frame.ages(Instant::now());
            for (event, value) in [
                (14, trace.time(frame.captured_at)),
                (15, u64::try_from(content.as_micros()).unwrap_or(u64::MAX)),
                (16, u64::try_from(residence.as_micros()).unwrap_or(u64::MAX)),
                (17, trace.time(frame.published_at)),
                (
                    18,
                    u64::try_from(frame.conversion.as_micros()).unwrap_or(u64::MAX),
                ),
            ] {
                trace.record(host_trace::Record {
                    event,
                    frame: frame.capture_id,
                    value,
                    repeat: frame.repeat,
                    ..Default::default()
                });
            }
        }
        let started = trace.map(|_| Instant::now());
        if frame.nv12.len() != self.encoder.frame_bytes() {
            use std::sync::atomic::Ordering;
            let dims = self.frame_dims.load(Ordering::Relaxed);
            let (width, height) = ((dims >> 32) as u32, dims as u32);
            let resized = crate::encode_windows::nv12_len(width, height)
                .map_err(|error| format!("mf encode: {error}"))?;
            if (width, height) == self.encoder.dimensions() || resized != frame.nv12.len() {
                // The frame matches neither the current nor the reported
                // mode: a stale pre-resize keepalive. Skip it instead of
                // killing the encoder worker.
                return Ok(Vec::new());
            }
            self.encoder
                .reconfigure(width, height)
                .map_err(|error| format!("mf resize: {error}"))?;
        }
        let result = self.encoder.encode_nv12(&frame.nv12, frame.captured_at);
        if let (Some(trace), Some(started)) = (trace, started) {
            trace.record(host_trace::Record {
                event: 40,
                frame: frame.capture_id,
                value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                repeat: frame.repeat,
                ..Default::default()
            });
        }
        result
            .map(|frames| {
                frames
                    .into_iter()
                    .map(|encoded| VideoFrame {
                        data: encoded.data,
                        is_key_frame: encoded.is_key_frame,
                        capture_at: encoded.capture_at,
                        encode_started_at: encoded.encode_started_at,
                        encode_completed_at: encoded.encode_completed_at,
                    })
                    .collect()
            })
            .map_err(|error| format!("mf encode: {error}"))
    }
}

#[cfg(target_os = "windows")]
fn spawn_windows_cursor<T: Send + 'static>(
    workers: &mut native_pipeline::Workers<T>,
    sender: SyncSender<MediaEvent>,
    source: &WindowsMediaSource,
) -> io::Result<()> {
    let origin_x = source.desktop_x;
    let origin_y = source.desktop_y;
    let mon_w = source.width;
    let mon_h = source.height;
    workers.spawn("maho-win-cursor", false, move |handoff| {
        let mut capture = crate::cursor::WindowsCursor::default();
        let mut previous = None;
        let mut pending_shape = None;
        while !handoff.is_stopped() {
            let started = Instant::now();
            let (cursor, shape_id, shape) = capture.sample(origin_x, origin_y, mon_w, mon_h);
            if shape.is_some() {
                pending_shape = shape;
            }
            let current = (cursor.x, cursor.y, cursor.cursor_type, shape_id);
            if previous != Some(current) || pending_shape.is_some() {
                match sender.try_send(MediaEvent::Cursor(cursor, shape_id, pending_shape.take())) {
                    Ok(()) => previous = Some(current),
                    Err(mpsc::TrySendError::Full(MediaEvent::Cursor(_, _, shape))) => {
                        pending_shape = shape
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                    Err(_) => unreachable!(),
                }
            }
            handoff.pace_until(started + Duration::from_nanos(1_000_000_000 / 240));
        }
    })
}

#[cfg(target_os = "windows")]
impl MediaSource for WindowsMediaSource {
    fn supports_cpu_scaling(&self) -> bool {
        false
    }

    fn output_target(&self) -> Option<TargetDisplay> {
        WindowsCapture::selected_output_metadata(self.display_index)
            .ok()
            .map(|meta| TargetDisplay {
                x: meta.desktop_x,
                y: meta.desktop_y,
                width: meta.pixel_width,
                height: meta.pixel_height,
            })
    }

    fn start_with_capabilities(
        &self,
        sender: SyncSender<MediaEvent>,
        _peer: Capabilities,
    ) -> Result<Box<dyn MediaHandle>, SessionError> {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

        let (codec_tx, codec_rx) = mpsc::sync_channel(1);
        let mut workers = native_pipeline::Workers::<WindowsRawFrame>::new();
        let first_keyframe_emitted = Arc::new(AtomicBool::new(false));
        let frame_dims = Arc::new(AtomicU64::new(
            (u64::from(self.width) << 32) | u64::from(self.height),
        ));
        {
            let sender = sender.clone();
            let first_keyframe_emitted = Arc::clone(&first_keyframe_emitted);
            let frame_dims = Arc::clone(&frame_dims);
            let display_index = self.display_index;
            let fps = self.fps.max(1);
            workers.spawn("maho-win-capture", true, move |handoff| {
                // SetThreadExecutionState is per-thread. Reset it on this same
                // owning thread on every exit, including initialization failure.
                struct KeepAwake;
                impl Drop for KeepAwake {
                    fn drop(&mut self) {
                        display_power::set_keep_awake(false);
                    }
                }
                display_power::set_keep_awake(true);
                let _awake = KeepAwake;
                let interval = Duration::from_micros(1_000_000 / u64::from(fps));
                let mut capture =
                    match WindowsCapture::new(display_index, Duration::from_millis(33)) {
                        Ok(capture) => capture,
                        Err(error) => {
                            native_media_error(&handoff, &sender, format!("dxgi init: {error}"));
                            return;
                        }
                    };
                // Share the immutable NV12 allocation with the keepalive cache.
                // Repeats retain the real original capture instant.
                let mut last_frame: Option<WindowsRawFrame> = None;
                let mut capture_id = 0_u64;
                let mut consecutive_failures = 0_u32;
                let mut last_emit = Instant::now();
                let mut jiggle_flip = false;
                let mut last_jiggle = Instant::now() - Duration::from_millis(700);
                while !handoff.is_stopped() {
                    let started = Instant::now();
                    if let Some(trace) = host_trace::enabled() {
                        trace.record(host_trace::Record {
                            event: 9,
                            frame: capture_id + 1,
                            ..Default::default()
                        });
                    }
                    match capture.acquire_next_frame(Duration::from_millis(33)) {
                        Ok(frame) => {
                            consecutive_failures = 0;
                            frame_dims.store(
                                (u64::from(frame.width) << 32) | u64::from(frame.height),
                                Ordering::Relaxed,
                            );
                            if let Some(trace) = host_trace::enabled() {
                                trace.record(host_trace::Record {
                                    event: 10,
                                    frame: capture_id + 1,
                                    value: trace.time(started),
                                    ..Default::default()
                                });
                            }
                            let captured_at = Instant::now();
                            let nv12 = match bgra_to_nv12(
                                frame.width,
                                frame.height,
                                &frame.bgra,
                                frame.stride as usize,
                            ) {
                                Ok(nv12) => nv12,
                                Err(error) => {
                                    native_media_error(
                                        &handoff,
                                        &sender,
                                        format!("nv12 conversion: {error}"),
                                    );
                                    return;
                                }
                            };
                            capture_id += 1;
                            let published_at = Instant::now();
                            let raw = WindowsRawFrame {
                                nv12: Arc::new(nv12),
                                captured_at,
                                published_at,
                                capture_id,
                                repeat: false,
                                conversion: published_at.saturating_duration_since(captured_at),
                            };
                            last_frame = Some(raw.clone());
                            if let Some(trace) = host_trace::enabled() {
                                trace.record(host_trace::Record {
                                    event: 11,
                                    frame: capture_id,
                                    value: trace.time(captured_at),
                                    ..Default::default()
                                });
                                trace.record(host_trace::Record {
                                    event: 12,
                                    frame: capture_id,
                                    value: trace.time(published_at),
                                    ..Default::default()
                                });
                            }
                            last_emit = started;
                            if !handoff.publish(raw) {
                                break;
                            }
                        }
                        Err(CaptureError::Timeout) => {
                            if (!first_keyframe_emitted.load(Ordering::Relaxed)
                                || last_frame.is_none())
                                && last_jiggle.elapsed() >= Duration::from_millis(300)
                            {
                                last_jiggle = started;
                                jiggle_flip = !jiggle_flip;
                                cursor_jiggle::nudge(jiggle_flip);
                            }
                            let keepalive_interval =
                                if first_keyframe_emitted.load(Ordering::Relaxed) {
                                    Duration::from_millis(500)
                                } else {
                                    Duration::from_millis(33)
                                };
                            if last_emit.elapsed() >= keepalive_interval {
                                if let Some(frame) = &last_frame {
                                    last_emit = started;
                                    if let Some(trace) = host_trace::enabled() {
                                        trace.record(host_trace::Record {
                                            event: 13,
                                            frame: frame.capture_id,
                                            value: trace.time(frame.captured_at),
                                            ..Default::default()
                                        });
                                    }
                                    if !handoff.publish(frame.repeated(Instant::now())) {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            use crate::windows_session::{capture_recovery, CaptureRecovery};

                            let failure = capture_failure_from_label(match &error {
                                CaptureError::AccessLost => "access_lost",
                                CaptureError::AccessDenied => "access_denied",
                                CaptureError::RefreshFailure => "refresh_failure",
                                _ => "capture",
                            });
                            warn!(%error, consecutive_failures, "DXGI capture interrupted");
                            while !handoff.is_stopped() {
                                match capture_recovery(failure, consecutive_failures) {
                                    CaptureRecovery::Reacquire(delay) => {
                                        thread::sleep(delay);
                                        if handoff.is_stopped() {
                                            break;
                                        }
                                        let rebuilt = WindowsCapture::new(
                                            display_index,
                                            Duration::from_millis(33),
                                        );
                                        consecutive_failures =
                                            consecutive_failures.saturating_add(1);
                                        match rebuilt {
                                            Ok(rebuilt) => {
                                                capture = rebuilt;
                                                break;
                                            }
                                            Err(error) => {
                                                warn!(%error, consecutive_failures, "DXGI reacquire failed");
                                            }
                                        }
                                    }
                                    CaptureRecovery::Abort => {
                                        native_media_error(
                                            &handoff,
                                            &sender,
                                            format!("dxgi: {error}"),
                                        );
                                        return;
                                    }
                                }
                            }
                            continue;
                        }
                    }
                    handoff.pace_until(started + interval);
                }
            })?;
        }
        if self.capture_audio {
            let sender = sender.clone();
            workers.spawn("maho-win-audio", false, move |handoff| {
                let mut capture =
                    match crate::audio_windows::WindowsAudioCapture::new() {
                        Ok(capture) => capture,
                        Err(error) => {
                            tracing::warn!(%error, "Windows audio loopback unavailable; continuing without audio");
                            return;
                        }
                    };
                info!("Windows audio loopback started");
                while !handoff.is_stopped() {
                    let pcm = capture.poll();
                    if pcm.is_empty() {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    let _ = sender.send(MediaEvent::TimedAudio { pcm, captured_at: Instant::now() });
                }
            })?;
        }
        let config = EncoderConfig {
            width: self.width,
            height: self.height,
            bitrate: self.bitrate,
            fps: self.fps.max(1),
            keyframe_interval: 0,
            preferred_codec: self.codec,
        };
        let cursor_sender = sender.clone();
        workers.spawn("maho-win-encode", true, move |handoff| {
            let frame_dims = Arc::clone(&frame_dims);
            let result = native_pipeline::run_encoder(
                &handoff,
                || {
                    MediaFoundationEncoder::new(config)
                        .map(|encoder| {
                            let _ = codec_tx.send(match encoder.codec() {
                                VideoCodec::Hevc => StreamCodecPreference::Hevc,
                                VideoCodec::H264 => StreamCodecPreference::H264,
                            });
                            WindowsSessionEncoder {
                                encoder,
                                frame_dims,
                            }
                        })
                        .map_err(|error| format!("mf init: {error}"))
                },
                |frame| {
                    let is_key = frame.is_key_frame;
                    let trace = host_trace::enabled();
                    let started = trace.map(|_| Instant::now());
                    if let Some(trace) = trace {
                        trace.record(host_trace::Record {
                            event: 23,
                            keyframe: is_key,
                            size: frame.data.len(),
                            ..Default::default()
                        });
                    }
                    let sent = sender.send(MediaEvent::Video(frame)).is_ok();
                    if let (Some(trace), Some(started)) = (trace, started) {
                        trace.record(host_trace::Record {
                            event: 24,
                            keyframe: is_key,
                            value: u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                            repeat: !sent,
                            ..Default::default()
                        });
                    }
                    if sent && is_key {
                        first_keyframe_emitted.store(true, Ordering::Relaxed);
                    }
                    sent
                },
            );
            if let Err(error) = result {
                native_media_error(&handoff, &sender, error);
            }
        })?;
        spawn_windows_cursor(&mut workers, cursor_sender, self)?;
        workers.handoff.activate();
        let codec = codec_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| SessionError::MediaStopped)?;
        Ok(Box::new(WindowsMediaHandle { workers, codec }))
    }
}

#[cfg(target_os = "windows")]
struct WindowsMediaHandle {
    codec: StreamCodecPreference,
    workers: native_pipeline::Workers<WindowsRawFrame>,
}

#[cfg(target_os = "windows")]
impl MediaHandle for WindowsMediaHandle {
    fn codec(&self) -> StreamCodecPreference {
        self.codec
    }
    fn force_key_frame(&self) -> Result<(), SessionError> {
        self.workers
            .handoff
            .control(true, None)
            .then_some(())
            .ok_or(SessionError::MediaStopped)
    }

    fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError> {
        self.workers
            .handoff
            .control(false, Some(bitrate))
            .then_some(())
            .ok_or(SessionError::MediaStopped)
    }

    fn stop(&mut self) {
        self.workers.stop();
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct LinuxMediaSource {
    fps: u32,
    width: u32,
    height: u32,
    bitrate: u32,
    capture_audio: bool,
    output_name: Option<String>,
    codec: LinuxVideoCodec,
    scale_permille: u16,
}

#[cfg(any(target_os = "linux", test))]
fn downscale_bgra_nearest(
    src: &[u8],
    src_stride: usize,
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
) -> Vec<u8> {
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return Vec::new();
    }
    let mut dst = vec![0u8; (dw as usize) * (dh as usize) * 4];
    for dy in 0..dh {
        let sy = ((dy as u64) * (sh as u64) / (dh as u64)) as u32;
        let src_row_start = (sy as usize) * src_stride;
        let dst_row_start = (dy as usize) * (dw as usize) * 4;
        for dx in 0..dw {
            let sx = ((dx as u64) * (sw as u64) / (dw as u64)) as u32;
            let src_px = src_row_start + (sx as usize) * 4;
            let dst_px = dst_row_start + (dx as usize) * 4;
            if src_px + 4 <= src.len() && dst_px + 4 <= dst.len() {
                dst[dst_px..dst_px + 4].copy_from_slice(&src[src_px..src_px + 4]);
            }
        }
    }
    dst
}

#[cfg(target_os = "linux")]
struct LinuxRawFrame {
    bgra: Vec<u8>,
    stride: usize,
    width: u32,
    height: u32,
    captured_at: Instant,
}

#[cfg(target_os = "linux")]
struct LinuxSessionEncoder {
    encoder: LinuxVideoEncoder,
    times: native_pipeline::FrameTimes,
}

#[cfg(target_os = "linux")]
impl LinuxSessionEncoder {
    fn output(
        &mut self,
        frames: Vec<crate::encode_linux::EncodedFrame>,
    ) -> Result<Vec<VideoFrame>, String> {
        let encode_completed_at = Instant::now();
        frames
            .into_iter()
            .map(|encoded| {
                let (capture_at, encode_started_at) =
                    self.times.take(encoded.pts).ok_or_else(|| {
                        format!("encoder output has unknown input PTS {}", encoded.pts)
                    })?;
                Ok(VideoFrame {
                    data: encoded.data,
                    is_key_frame: encoded.is_key_frame,
                    capture_at,
                    encode_started_at,
                    encode_completed_at,
                })
            })
            .collect()
    }
}

#[cfg(target_os = "linux")]
impl native_pipeline::Encoder<LinuxRawFrame> for LinuxSessionEncoder {
    type Output = VideoFrame;
    type Error = String;

    fn force_keyframe(&mut self) {
        self.encoder.force_key_frame();
    }

    fn bitrate(&mut self, bitrate: u32) -> Result<Vec<VideoFrame>, String> {
        let frames = self
            .encoder
            .update_bitrate(bitrate as usize)
            .map_err(|error| format!("encoder bitrate: {error}"))?;
        self.output(frames)
    }

    fn encode(&mut self, frame: LinuxRawFrame) -> Result<Vec<VideoFrame>, String> {
        let mut frames = Vec::new();
        if (frame.width, frame.height) != self.encoder.dimensions() {
            // Capture resolution changed: adopt it instead of failing the
            // pipeline on every subsequent InvalidFrame.
            let drained = self
                .encoder
                .reconfigure(frame.width, frame.height)
                .map_err(|error| format!("encoder resize: {error}"))?;
            frames = self.output(drained)?;
        }
        // Record the input only after the encoder accepts it: a rejected
        // frame must not advance FrameTimes past the encoder's PTS counter,
        // and its pending entry must not leak.
        self.times.submitted(frame.captured_at, Instant::now());
        let encoded = self
            .encoder
            .encode_bgra(&frame.bgra, frame.stride)
            .map_err(|error| format!("encode: {error}"))?;
        frames.extend(self.output(encoded)?);
        Ok(frames)
    }
}

#[cfg(target_os = "linux")]
impl MediaSource for LinuxMediaSource {
    fn start_with_capabilities(
        &self,
        sender: SyncSender<MediaEvent>,
        peer: Capabilities,
    ) -> Result<Box<dyn MediaHandle>, SessionError> {
        let metadata_cursor = peer.contains(Capabilities::CURSOR_METADATA);
        let (codec_tx, codec_rx) = mpsc::sync_channel(1);
        let mut workers = native_pipeline::Workers::<LinuxRawFrame>::new();
        {
            let sender = sender.clone();
            let fps = self.fps.max(1);
            let scale_permille = self.scale_permille;
            let effective = reprobe_output_name(self.output_name.as_deref());
            let output_name = effective;
            workers.spawn("maho-linux-capture", true, move |handoff| {
                if let Err(error) = linux_capture_pipeline::run(
                    &handoff,
                    &sender,
                    output_name,
                    fps,
                    scale_permille,
                    metadata_cursor,
                ) {
                    if !handoff.is_stopped() {
                        native_media_error(&handoff, &sender, error);
                    }
                }
            })?;
        }
        {
            let sender = sender.clone();
            let config = LinuxEncoderConfig {
                width: self.width,
                height: self.height,
                bitrate: self.bitrate as usize,
                fps: self.fps.max(1),
                keyframe_interval: 0,
                preferred_codec: self.codec,
            };
            workers.spawn("maho-linux-encode", true, move |handoff| {
                let result = native_pipeline::run_encoder(
                    &handoff,
                    || {
                        LinuxVideoEncoder::new(config)
                            .map(|encoder| {
                                let _ = codec_tx.send(match encoder.codec() {
                                    LinuxVideoCodec::Hevc => StreamCodecPreference::Hevc,
                                    LinuxVideoCodec::H264 => StreamCodecPreference::H264,
                                });
                                LinuxSessionEncoder {
                                    encoder,
                                    times: native_pipeline::FrameTimes::new(),
                                }
                            })
                            .map_err(|error| format!("encoder init: {error}"))
                    },
                    |frame| sender.send(MediaEvent::Video(frame)).is_ok(),
                );
                if let Err(error) = result {
                    native_media_error(&handoff, &sender, error);
                }
            })?;
        }
        if self.capture_audio {
            workers.spawn("maho-linux-audio", false, move |handoff| {
                run_linux_audio(handoff, sender);
            })?;
        }
        workers.handoff.activate();
        let codec = codec_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| SessionError::MediaStopped)?;
        Ok(Box::new(LinuxMediaHandle { workers, codec }))
    }
}

#[cfg(target_os = "linux")]
fn run_linux_audio<T>(handoff: Arc<native_pipeline::Handoff<T>>, sender: SyncSender<MediaEvent>) {
    use crate::audio_linux::LinuxAudioCapture;
    let mut retry = Duration::from_millis(250);
    while !handoff.is_stopped() {
        let mut capture = match LinuxAudioCapture::open_cancellable(handoff.cancellation()) {
            Ok(capture) => capture,
            Err(error) => {
                warn!(%error, "Audio capture unavailable");
                handoff.report_audio_unavailable(error.to_string());
                handoff.pace_until(Instant::now() + retry);
                retry = (retry * 2).min(Duration::from_secs(5));
                continue;
            }
        };
        let slot = Arc::new(native_pipeline::LatestAudio::default());
        let mut samples = [0.0_f32; 1920];
        while !handoff.is_stopped() {
            match capture.read_interleaved_f32_cancellable(
                &mut samples,
                handoff.cancellation(),
                Duration::from_millis(50),
            ) {
                Ok(0) => {}
                Ok(count) => {
                    retry = Duration::from_millis(250);
                    let captured_at = Instant::now();
                    let mut pcm = Vec::with_capacity(count * 4);
                    for sample in &samples[..count] {
                        pcm.extend_from_slice(&sample.to_le_bytes());
                    }
                    if slot.publish(native_pipeline::AudioBlock { pcm, captured_at }) {
                        match sender.try_send(MediaEvent::NativeAudio(Arc::clone(&slot))) {
                            Ok(()) => {}
                            Err(mpsc::TrySendError::Full(_)) => slot.notification_rejected(),
                            Err(mpsc::TrySendError::Disconnected(_)) => {
                                handoff.stop();
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    if !handoff.is_stopped() {
                        warn!(%error, "Audio capture ended");
                        handoff.report_audio_unavailable(error.to_string());
                    }
                    break;
                }
            }
        }

        // Drop/reap before retry so no failed recorder survives the reopen.
        drop(capture);
        handoff.pace_until(Instant::now() + retry);
        retry = (retry * 2).min(Duration::from_secs(5));
    }
}

#[cfg(target_os = "linux")]
struct LinuxMediaHandle {
    codec: StreamCodecPreference,
    workers: native_pipeline::Workers<LinuxRawFrame>,
}

#[cfg(target_os = "linux")]
impl MediaHandle for LinuxMediaHandle {
    fn take_audio_error(&self) -> Option<String> {
        self.workers.handoff.take_audio_error()
    }
    fn codec(&self) -> StreamCodecPreference {
        self.codec
    }
    fn force_key_frame(&self) -> Result<(), SessionError> {
        self.workers
            .handoff
            .control(true, None)
            .then_some(())
            .ok_or(SessionError::MediaStopped)
    }

    fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError> {
        self.workers
            .handoff
            .control(false, Some(bitrate))
            .then_some(())
            .ok_or(SessionError::MediaStopped)
    }

    fn stop(&mut self) {
        self.workers.stop();
    }
}

/// BT.601 limited-range BGRA8 -> NV12 conversion for tightly packed input.
#[cfg(target_os = "windows")]
fn bgra_to_nv12(
    width: u32,
    height: u32,
    bgra: &[u8],
    stride: usize,
) -> Result<Vec<u8>, &'static str> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
        return Err("display dimensions must be non-zero and even");
    }
    if bgra.len() < stride * h || stride < w * 4 {
        return Err("bgra buffer smaller than stride * height");
    }
    let y_plane = w * h;
    let mut nv12 = vec![128u8; y_plane + y_plane / 2];
    let (y_plane, uv_plane) = nv12.split_at_mut(y_plane);
    for row in 0..h {
        let src = &bgra[row * stride..row * stride + w * 4];
        let y_row = &mut y_plane[row * w..(row + 1) * w];
        for col in (0..w).step_by(2) {
            let (b0, g0, r0) = (
                src[col * 4] as u32,
                src[col * 4 + 1] as u32,
                src[col * 4 + 2] as u32,
            );
            let (b1, g1, r1) = (
                src[col * 4 + 4] as u32,
                src[col * 4 + 5] as u32,
                src[col * 4 + 6] as u32,
            );
            y_row[col] = ((77 * r0 + 150 * g0 + 29 * b0) >> 8) as u8;
            y_row[col + 1] = ((77 * r1 + 150 * g1 + 29 * b1) >> 8) as u8;
            let (b_avg, g_avg, r_avg) = ((b0 + b1) / 2, (g0 + g1) / 2, (r0 + r1) / 2);
            let uv_index = (row / 2) * w + col;
            uv_plane[uv_index] =
                (128 + ((-43 * r_avg as i32 - 85 * g_avg as i32 + 128 * b_avg as i32) >> 8)) as u8;
            uv_plane[uv_index + 1] =
                (128 + ((128 * r_avg as i32 - 107 * g_avg as i32 - 21 * b_avg as i32) >> 8)) as u8;
        }
    }
    Ok(nv12)
}

/// Requested pipeline configuration passed to the restart factory. Only the
/// Linux CPU source reads every field (the test factory ignores them), and
/// other platforms reject restarts.
#[cfg_attr(any(not(target_os = "linux"), test), allow(dead_code))]
struct StreamRestart {
    width: u32,
    height: u32,
    scale_permille: u16,
    fps: u32,
    codec: StreamCodecPreference,
    output_name: Option<String>,
    bitrate: u32,
}

#[path = "transport_recovery.rs"]
mod transport_recovery;
#[cfg(test)]
#[path = "transport_recovery_tests.rs"]
mod transport_recovery_tests;
#[path = "udp_input.rs"]
mod udp_input;
#[path = "udp_recovery.rs"]
mod udp_recovery;

type RestartFactory = Arc<dyn Fn(&StreamRestart) -> Arc<dyn MediaSource> + Send + Sync>;

pub struct HostServer {
    guests: Arc<GuestRegistry>,
    config: HostConfig,
    tcp_listener: TcpListener,
    udp_socket: UdpSocket,
    /// Sole owner of shared-socket reads: registered peers' inboxes plus the
    /// admission inbox for unknown endpoints.
    udp_recovery: Arc<Mutex<udp_recovery::UdpRecoveryRoutes>>,
    media_source: Arc<dyn MediaSource>,
    restart_factory: Option<RestartFactory>,
    #[cfg(test)]
    synthetic_starts: Option<Arc<std::sync::atomic::AtomicUsize>>,
    lockout: Arc<Mutex<BootstrapLockout>>,
    pairing_deadline: Option<Instant>,
    bootstrap_identity: Option<PskIdentity>,
    preauth_timeout: Duration,
    pub session_stall_timeout: Duration,
    _advertisement: Option<ferryx_rd_net::discovery::ServiceAdvertiser>,
    #[cfg(all(test, target_os = "linux"))]
    prepared_admission_input: Mutex<Option<LinuxInputInjector>>,
}

#[cfg(test)]
thread_local! {
    static BOOTSTRAP_DERIVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn derive_bootstrap_identity(pin: &str) -> Result<PskIdentity, TlsPskError> {
    #[cfg(test)]
    BOOTSTRAP_DERIVATIONS.with(|count| count.set(count.get() + 1));
    PskIdentity::bootstrap(pin)
}

fn host_guest_capabilities() -> Capabilities {
    let mut capabilities = Capabilities::HOST_STATUS
        | Capabilities::SESSION_ENDED
        | Capabilities::GUEST_PERMISSIONS
        | Capabilities::PAIRING_TRUST
        | Capabilities::RTT_PROBE
        | Capabilities::UDP_INPUT
        | Capabilities::FEC_NACK
        | Capabilities::AUDIO_SOURCE_TIME
        | Capabilities::AUDIO_OPUS
        | Capabilities::CLOCK_CORRELATION
        | Capabilities::INPUT_ECHO;
    #[cfg(target_os = "windows")]
    {
        capabilities |= Capabilities::SECURE_ATTENTION;
        if crate::gamepad_windows::available() {
            capabilities |= Capabilities::GAMEPAD;
        }
    }
    #[cfg(target_os = "linux")]
    {
        capabilities |= Capabilities::CURSOR_METADATA | Capabilities::CURSOR_SHAPE;
    }
    #[cfg(target_os = "linux")]
    if crate::gamepad_linux::available() {
        capabilities |= Capabilities::GAMEPAD;
    }
    if cfg!(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos"
    )) {
        capabilities |= Capabilities::TEXT_CLIPBOARD_SYNC;
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        capabilities |= Capabilities::CLIPBOARD_IMAGE | Capabilities::CURSOR_SHAPE;
    }
    #[cfg(target_os = "linux")]
    if crate::clipboard_linux::ClipboardBackend::detect()
        .is_ok_and(|b| b == crate::clipboard_linux::ClipboardBackend::Wayland)
    {
        capabilities |= Capabilities::CLIPBOARD_IMAGE;
    }
    capabilities
}

fn send_session_end(
    stream: &mut ferryx_rd_net::TlsPskStream<TcpStream>,
    peer: Capabilities,
    reason: SessionEndReason,
) -> Result<(), SessionError> {
    send_tcp_control(
        stream,
        if peer.contains(Capabilities::SESSION_ENDED) {
            ControlMessage::SessionEnded(SessionEnded { reason })
        } else {
            ControlMessage::Disconnect
        },
    )
}

fn send_permissions(
    stream: &mut ferryx_rd_net::TlsPskStream<TcpStream>,
    peer: Capabilities,
    permissions: GuestPermissions,
) -> Result<(), SessionError> {
    if peer.contains(Capabilities::GUEST_PERMISSIONS) {
        send_tcp_control(stream, ControlMessage::GuestPermissions(permissions))?;
    } else if peer.contains(Capabilities::STREAM_OPTIONS) {
        send_tcp_control(
            stream,
            ControlMessage::HostPolicy(HostPolicy {
                allow_input: permissions.keyboard && permissions.mouse,
                allow_clipboard: permissions.clipboard,
            }),
        )?;
    }
    Ok(())
}

impl HostServer {
    pub fn guest_registry(&self) -> Arc<GuestRegistry> {
        self.guests.clone()
    }

    pub fn set_secure_attention_handler(
        &self,
        handler: Arc<dyn crate::guests::SecureAttentionHandler>,
    ) {
        self.guests.set_secure_attention_handler(handler);
    }

    pub fn sessions(&self) -> Vec<GuestSession> {
        self.guests.sessions()
    }

    pub fn kick_session(&self, id: &str) -> bool {
        self.guests.kick(id)
    }

    pub fn kick_all_sessions(&self) -> usize {
        self.guests.kick_all()
    }

    pub fn subscribe_events(&self) -> Receiver<crate::guests::HostEvent> {
        self.guests.subscribe()
    }

    pub fn pending_requests(&self) -> Vec<PendingRequest> {
        self.guests.pending_requests()
    }

    pub fn approve_request(&self, id: &str, trust: PairingTrust) -> bool {
        self.guests.respond(id, Some(trust))
    }

    pub fn deny_request(&self, id: &str) -> bool {
        self.guests.respond(id, None)
    }

    pub fn set_guest_permissions(
        &self,
        pairing_id: &str,
        permissions: GuestPermissions,
    ) -> Result<(), SessionError> {
        self.guests.set_guest_permissions(pairing_id, permissions)
    }

    fn permission_ceiling(&self, host_caps: Capabilities, clipboard: bool) -> GuestPermissions {
        let policy = self.config.host_policy();
        GuestPermissions {
            keyboard: policy.allow_input,
            mouse: policy.allow_input,
            gamepad: policy.allow_input && host_caps.contains(Capabilities::GAMEPAD),
            clipboard: policy.allow_clipboard
                && clipboard
                && cfg!(any(
                    target_os = "linux",
                    target_os = "windows",
                    target_os = "macos"
                )),
            audio: self.config.capture_audio,
        }
    }

    pub fn bind(mut config: HostConfig) -> Result<Self, SessionError> {
        let fps = if config.frames_per_second == 0 {
            60
        } else {
            config.frames_per_second
        };
        config.bitrate = starting_bitrate(
            config.display.pixel_width,
            config.display.pixel_height,
            fps,
            StreamCodecPreference::Auto,
            config.bitrate,
        );
        let media_source = Self::default_media_source(&config, fps)?;
        Self::bind_with_media_advertising(config, media_source, true)
    }

    /// Same as [`HostServer::bind`] but never starts LAN discovery advertising. Embedders that
    /// publish reachability themselves (Ferryx paired hosts) use this.
    pub fn bind_unadvertised(mut config: HostConfig) -> Result<Self, SessionError> {
        let fps = if config.frames_per_second == 0 {
            60
        } else {
            config.frames_per_second
        };
        config.bitrate = starting_bitrate(
            config.display.pixel_width,
            config.display.pixel_height,
            fps,
            StreamCodecPreference::Auto,
            config.bitrate,
        );
        let media_source = Self::default_media_source(&config, fps)?;
        Self::bind_with_media_advertising(config, media_source, false)
    }

    #[cfg(target_os = "macos")]
    fn default_media_source(
        config: &HostConfig,
        fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        Ok(Arc::new(MacMediaSource {
            config: CaptureConfig {
                display_id: None,
                width: config.display.pixel_width,
                height: config.display.pixel_height,
                frames_per_second: fps,
                capture_audio: config.capture_audio,
            },
            codec: VtCodec::Hevc,
            encoder: EncoderConfig {
                width: config.display.pixel_width,
                height: config.display.pixel_height,
                frames_per_second: fps,
                bitrate: config.bitrate,
                key_frame_interval: fps.saturating_mul(10),
            },
        }))
    }

    #[cfg(target_os = "linux")]
    fn default_media_source(
        config: &HostConfig,
        fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        let cpu = LinuxMediaSource {
            fps,
            width: config.display.pixel_width,
            height: config.display.pixel_height,
            bitrate: config.bitrate,
            capture_audio: config.capture_audio,
            output_name: config.output_name.clone(),
            codec: LinuxVideoCodec::Hevc,
            scale_permille: 1000,
        };
        Ok(select_linux_source(cpu))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn default_media_source(
        _config: &HostConfig,
        _fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        Err(SessionError::Store(
            "no media source is wired for this platform yet".into(),
        ))
    }

    #[cfg(target_os = "windows")]
    fn default_media_source(
        config: &HostConfig,
        fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        let cpu = WindowsMediaSource {
            display_index: 0,
            fps,
            width: config.display.pixel_width,
            height: config.display.pixel_height,
            bitrate: config.bitrate,
            codec: VideoCodec::H264,
            capture_audio: config.capture_audio,
            desktop_x: config.display.desktop_x,
            desktop_y: config.display.desktop_y,
        };
        Ok(select_windows_source(
            cpu,
            None,
            StreamCodecPreference::Auto,
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn default_media_source(
        _config: &HostConfig,
        _fps: u32,
    ) -> Result<Arc<dyn MediaSource>, SessionError> {
        Err(SessionError::Store(
            "no media source is wired for this platform yet".into(),
        ))
    }

    #[cfg(test)]
    fn bind_synthetic(config: HostConfig, frame_count: u32) -> Result<Self, SessionError> {
        let restart_starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let synthetic = Arc::new(SyntheticMediaSource {
            frame_count,
            restart_starts: Arc::clone(&restart_starts),
        });
        let factory_source = Arc::clone(&synthetic);
        let factory: RestartFactory =
            Arc::new(move |_restart| Arc::clone(&factory_source) as Arc<dyn MediaSource>);
        let mut server =
            Self::bind_with_media_advertising_and_factory(config, synthetic, Some(factory), false)?;
        server.synthetic_starts = Some(restart_starts);
        Ok(server)
    }

    #[cfg(test)]
    fn bind_with_media(
        config: HostConfig,
        media_source: Arc<dyn MediaSource>,
    ) -> Result<Self, SessionError> {
        Self::bind_with_media_advertising_and_factory(config, media_source, None, false)
    }

    fn bind_with_media_advertising(
        config: HostConfig,
        media_source: Arc<dyn MediaSource>,
        advertise: bool,
    ) -> Result<Self, SessionError> {
        #[cfg(all(target_os = "linux", not(test)))]
        let factory: Option<RestartFactory> = {
            let config_clone = config.clone();
            Some(Arc::new(move |restart: &StreamRestart| {
                let preferred_codec = match restart.codec {
                    StreamCodecPreference::H264 => LinuxVideoCodec::H264,
                    StreamCodecPreference::Hevc | StreamCodecPreference::Auto => {
                        LinuxVideoCodec::Hevc
                    }
                };
                select_linux_source(LinuxMediaSource {
                    fps: restart.fps,
                    width: restart.width,
                    height: restart.height,
                    bitrate: restart.bitrate,
                    capture_audio: config_clone.capture_audio,
                    output_name: restart.output_name.clone(),
                    codec: preferred_codec,
                    scale_permille: restart.scale_permille,
                })
            }))
        };
        #[cfg(all(target_os = "macos", not(test)))]
        let factory: Option<RestartFactory> = {
            let config = config.clone();
            Some(Arc::new(move |restart: &StreamRestart| {
                Arc::new(MacMediaSource {
                    config: CaptureConfig {
                        display_id: restart
                            .output_name
                            .as_deref()
                            .and_then(|id| id.parse().ok()),
                        width: restart.width,
                        height: restart.height,
                        frames_per_second: restart.fps,
                        capture_audio: config.capture_audio,
                    },
                    codec: if restart.codec == StreamCodecPreference::H264 {
                        VtCodec::H264
                    } else {
                        VtCodec::Hevc
                    },
                    encoder: EncoderConfig {
                        width: restart.width,
                        height: restart.height,
                        frames_per_second: restart.fps,
                        bitrate: restart.bitrate,
                        key_frame_interval: restart.fps.saturating_mul(10),
                    },
                })
            }))
        };
        #[cfg(all(target_os = "windows", not(test)))]
        let factory: Option<RestartFactory> = {
            let config = config.clone();
            Some(Arc::new(move |restart: &StreamRestart| {
                let display_index = crate::capture_windows::enumerate_displays()
                    .ok()
                    .and_then(|list| {
                        list.iter().position(|display| {
                            Some(display.name.as_str()) == restart.output_name.as_deref()
                        })
                    })
                    .unwrap_or(0);
                let meta = WindowsCapture::selected_output_metadata(display_index).ok();
                let target =
                    (restart.scale_permille != 1000).then_some((restart.width, restart.height));
                select_windows_source(
                    WindowsMediaSource {
                        display_index,
                        fps: restart.fps,
                        width: meta.as_ref().map_or(restart.width, |m| m.pixel_width),
                        height: meta.as_ref().map_or(restart.height, |m| m.pixel_height),
                        bitrate: restart.bitrate,
                        codec: if restart.codec == StreamCodecPreference::Hevc {
                            VideoCodec::Hevc
                        } else {
                            VideoCodec::H264
                        },
                        capture_audio: config.capture_audio,
                        desktop_x: meta.as_ref().map_or(0, |m| m.desktop_x),
                        desktop_y: meta.as_ref().map_or(0, |m| m.desktop_y),
                    },
                    target,
                    restart.codec,
                )
            }))
        };
        #[cfg(not(any(
            all(target_os = "linux", not(test)),
            all(target_os = "macos", not(test)),
            all(target_os = "windows", not(test))
        )))]
        let factory: Option<RestartFactory> = None;

        Self::bind_with_media_advertising_and_factory(config, media_source, factory, advertise)
    }

    fn bind_with_media_advertising_and_factory(
        config: HostConfig,
        media_source: Arc<dyn MediaSource>,
        restart_factory: Option<RestartFactory>,
        advertise: bool,
    ) -> Result<Self, SessionError> {
        if config
            .bootstrap_pin
            .as_ref()
            .is_some_and(|pin| pin.len() != 8 || !pin.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(SessionError::InvalidPin);
        }
        let tcp_listener = TcpListener::bind(config.tcp_addr)?;
        let udp_socket = UdpSocket::bind(config.udp_addr)?;
        let _ = rustix::net::sockopt::set_socket_recv_buffer_size(&udp_socket, 4 * 1024 * 1024);
        if let Err(error) =
            rustix::net::sockopt::set_socket_send_buffer_size(&udp_socket, 4 * 1024 * 1024)
        {
            warn!(%error, "unable to request host UDP send buffer");
        }
        match rustix::net::sockopt::socket_send_buffer_size(&udp_socket) {
            Ok(bytes) => info!(bytes, "effective host UDP send buffer"),
            Err(error) => warn!(%error, "cannot read effective host UDP send buffer"),
        }
        if let Err(error) = ferryx_rd_net::udp_gcm::configure_media_qos(&udp_socket, 34) {
            warn!(%error, "host media DSCP AF41 request failed");
        }
        udp_socket.set_nonblocking(true)?;
        let lockout = Arc::new(Mutex::new(BootstrapLockout::default()));
        let bootstrap_identity = config
            .bootstrap_pin
            .as_deref()
            .map(derive_bootstrap_identity)
            .transpose()?;
        let pairing_deadline = config
            .bootstrap_pin
            .as_ref()
            .map(|_| Instant::now() + config.pairing_window);
        if pairing_deadline.is_some() {
            lockout.lock().expect("lockout poisoned").begin_pairing();
        }
        let bound_tcp = tcp_listener.local_addr()?;
        let bound_udp = udp_socket.local_addr()?;
        let _advertisement = if advertise {
            match ferryx_rd_net::discovery::ServiceAdvertiser::start(
                &config.host_name,
                bound_tcp.port(),
                bound_udp.port(),
                bound_tcp,
            ) {
                Ok(adv) => {
                    tracing::info!(
                        host = %config.host_name,
                        tcp = %bound_tcp,
                        udp = %bound_udp,
                        "Started LAN discovery advertisement"
                    );
                    Some(adv)
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to start LAN discovery advertisement: {e}. Direct host connectivity remains usable."
                    );
                    None
                }
            }
        } else {
            None
        };
        let guests = Arc::new(GuestRegistry::with_store(config.pairing_store.clone()));
        #[cfg(target_os = "windows")]
        guests
            .set_secure_attention_handler(Arc::new(crate::service_windows::SecureAttentionBroker));
        Ok(Self {
            guests,
            config,
            tcp_listener,
            udp_socket,
            udp_recovery: Arc::default(),
            media_source,
            restart_factory,
            #[cfg(test)]
            synthetic_starts: None,
            lockout,
            pairing_deadline,
            bootstrap_identity,
            preauth_timeout: Duration::from_secs(10),
            session_stall_timeout: SESSION_STALL_TIMEOUT,
            _advertisement,
            #[cfg(all(test, target_os = "linux"))]
            prepared_admission_input: Mutex::new(None),
        })
    }

    pub fn is_advertising(&self) -> bool {
        self._advertisement.is_some()
    }

    pub fn tcp_addr(&self) -> Result<SocketAddr, SessionError> {
        Ok(self.tcp_listener.local_addr()?)
    }

    pub fn udp_addr(&self) -> Result<SocketAddr, SessionError> {
        Ok(self.udp_socket.local_addr()?)
    }

    pub fn serve(self) -> Result<(), SessionError> {
        self.serve_with_stop(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )))
    }

    pub fn serve_with_stop(
        self,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), SessionError> {
        self.tcp_listener.set_nonblocking(true)?;
        // The accept loop dispatches each connection to a dedicated thread so that
        // a stalled session or TLS handshake never parks the accept thread.
        let bound = self
            .tcp_listener
            .local_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| "?".to_owned());
        crate::host_log(&format!("server: listening on {bound}"));
        let this = Arc::new(self);
        let active_connections = Arc::new(AtomicUsize::new(0));

        while !stop.load(Ordering::Relaxed) {
            SERVER_LOOP_ITERATIONS.fetch_add(1, Ordering::Relaxed);
            match this.tcp_listener.accept() {
                Ok((tcp, peer)) => {
                    let admission_deadline = Instant::now() + this.preauth_timeout;
                    let mut count = active_connections.load(Ordering::Relaxed);
                    let admitted = loop {
                        if count >= MAX_CONCURRENT_CONNECTIONS {
                            break false;
                        }
                        match active_connections.compare_exchange_weak(
                            count,
                            count + 1,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break true,
                            Err(observed) => count = observed,
                        }
                    };
                    if !admitted {
                        warn!(
                            %peer,
                            max = MAX_CONCURRENT_CONNECTIONS,
                            "server: max concurrent connections reached; dropping connection"
                        );
                        continue;
                    }
                    let permit = ConnectionPermit(Arc::clone(&active_connections));
                    let server = Arc::clone(&this);
                    let stop_flag = Arc::clone(&stop);
                    let spawn_res = thread::Builder::new()
                        .name("ferryx-rd-host-session".into())
                        .spawn(move || {
                            let _permit = permit;
                            let _ = server.serve_connection(
                                tcp,
                                peer,
                                admission_deadline,
                                Some(stop_flag.as_ref()),
                            );
                        });
                    if let Err(error) = spawn_res {
                        warn!(%peer, %error, "server: failed to spawn session thread");
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                Err(e) => return Err(SessionError::Io(e)),
            }
        }
        Ok(())
    }

    fn serve_connection(
        &self,
        tcp: TcpStream,
        peer: SocketAddr,
        admission_deadline: Instant,
        stop: Option<&AtomicBool>,
    ) -> Result<(), SessionError> {
        let accept_started = Instant::now();
        let _ = tcp.set_nonblocking(false);
        tcp.set_nodelay(true)?;
        SERVER_ACCEPTS.fetch_add(1, Ordering::Relaxed);
        crate::host_log(&format!("server: accepted {peer}"));
        let tls_server = ferryx_rd_net::tls_psk::TlsPskServer::new(self.current_psks()?)?;
        match tls_server.accept_stream_until(tcp, admission_deadline) {
            Ok(stream) => {
                SERVER_TLS_SUCCESSES.fetch_add(1, Ordering::Relaxed);
                crate::host_log(&format!(
                    "server: TLS established {peer} in {:?}",
                    accept_started.elapsed()
                ));
                let finished = Arc::new(AtomicBool::new(false));
                let _finished_guard = FinishedGuard(Arc::clone(&finished));
                let watch = Arc::new(SessionWatch::new(self.session_stall_timeout));
                let pipeline = PipelineSlot::default();
                let watchdog_finished = Arc::clone(&finished);
                let watchdog_watch = Arc::clone(&watch);
                let watchdog_pipeline = Arc::clone(&pipeline);
                let cloned_tcp = stream.ssl_stream().get_ref().try_clone()?;
                let timeout = watch.timeout();
                let poll = (timeout / 4).clamp(Duration::from_millis(50), Duration::from_secs(1));
                let watchdog_spawn = thread::Builder::new()
                    .name("ferryx-rd-host-watchdog".into())
                    .spawn(move || loop {
                        thread::sleep(poll);
                        let is_fin = watchdog_finished.load(Ordering::Relaxed);
                        let idle = watchdog_watch.idle(Instant::now());
                        if watchdog_should_abort(is_fin, idle, watchdog_watch.timeout()) {
                            warn!(
                                %peer,
                                ?idle,
                                "session watchdog aborted a stalled session"
                            );
                            let _ = cloned_tcp.shutdown(std::net::Shutdown::Both);
                            watchdog_watch.abort();
                            // Shutting down the socket cannot interrupt a thread
                            // parked in an unrelated syscall, so its pipeline is
                            // stopped here instead of waiting for that thread to
                            // unwind and drop it.
                            stop_pipeline(&watchdog_pipeline);
                            break;
                        }
                        if is_fin {
                            break;
                        }
                    });
                if let Err(error) = watchdog_spawn {
                    warn!(%peer, %error, "server: failed to spawn watchdog thread");
                }
                if let Err(error) = self.handle_connection(
                    stream,
                    peer,
                    admission_deadline,
                    stop,
                    &watch,
                    &pipeline,
                ) {
                    crate::host_log(&format!(
                        "server: connection ended {peer} after {:?}: {error}",
                        accept_started.elapsed()
                    ));
                    tracing::warn!(%error, "connection ended with an error");
                }
            }
            Err(error) => {
                crate::host_log(&format!(
                    "server: TLS failed {peer} after {:?}: {error}",
                    accept_started.elapsed()
                ));
                let locked = self
                    .lockout
                    .lock()
                    .expect("lockout poisoned")
                    .record_failure(Instant::now());
                if locked {
                    tracing::warn!("bootstrap TLS path locked after repeated failures");
                }
            }
        }
        Ok(())
    }

    pub fn serve_n(&self, connection_count: usize) -> Result<(), SessionError> {
        for _ in 0..connection_count {
            self.serve_next()?;
        }
        Ok(())
    }

    fn serve_next(&self) -> Result<(), SessionError> {
        let (tcp, peer) = self.tcp_listener.accept()?;
        let admission_deadline = Instant::now() + self.preauth_timeout;
        tcp.set_nodelay(true)?;
        let tls_server = TlsPskServer::new(self.current_psks()?)?;
        match tls_server.accept_stream_until(tcp, admission_deadline) {
            Ok(stream) => {
                let watch = SessionWatch::new(self.session_stall_timeout);
                let pipeline = PipelineSlot::default();
                self.handle_connection(stream, peer, admission_deadline, None, &watch, &pipeline)
            }
            Err(error) => {
                let locked = self
                    .lockout
                    .lock()
                    .expect("lockout poisoned")
                    .record_failure(Instant::now());
                if locked {
                    warn!("bootstrap TLS path locked after repeated failures");
                }
                Err(SessionError::Tls(error))
            }
        }
    }

    fn current_psks(&self) -> Result<Vec<PskIdentity>, SessionError> {
        let mut psks = self
            .config
            .pairing_store
            .load_all()?
            .into_iter()
            .filter(|record| {
                self.config
                    .pairing_store
                    .authorization(&record.id)
                    .is_ok_and(|(trust, _)| guests::valid_trust(trust, unix_ms_u64()))
            })
            .map(|record| PskIdentity::pairing(&record.id, &record.key))
            .collect::<Result<Vec<_>, _>>()?;
        let pairing_active = self
            .pairing_deadline
            .is_some_and(|deadline| Instant::now() < deadline)
            && self
                .lockout
                .lock()
                .expect("lockout poisoned")
                .is_allowed(Instant::now());
        if pairing_active {
            if let Some(identity) = &self.bootstrap_identity {
                psks.push(identity.clone());
            }
        }
        if psks.is_empty() {
            let mut disabled_key = [0_u8; 32];
            rand::rng().fill_bytes(&mut disabled_key);
            psks.push(PskIdentity::new("maho-disabled", disabled_key.to_vec())?);
        }
        Ok(psks)
    }

    fn is_pairing_active(&self) -> bool {
        self.pairing_deadline
            .is_some_and(|deadline| Instant::now() < deadline)
            && self
                .lockout
                .lock()
                .expect("lockout poisoned")
                .is_allowed(Instant::now())
    }

    fn handle_connection(
        &self,
        mut stream: ferryx_rd_net::TlsPskStream<TcpStream>,
        tcp_peer: SocketAddr,
        admission_deadline: Instant,
        stop: Option<&std::sync::atomic::AtomicBool>,
        watch: &SessionWatch,
        pipeline: &PipelineSlot,
    ) -> Result<(), SessionError> {
        stream
            .ssl_stream_mut()
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(5)))?;
        let negotiated_identity = stream
            .negotiated_identity()
            .and_then(|identity| std::str::from_utf8(identity).ok())
            .unwrap_or_default()
            .to_owned();
        let mut state = SessionState::PreAuth;
        let mut granted_pairing_id: Option<String> = None;
        let mut provisional: Option<PairingRecord> = None;
        let mut session_trust = PairingTrust::GuestOnce;
        let mut peer_caps = Capabilities::empty();
        let mut guest_guard = None;
        let mut guest_commands: Option<Receiver<GuestCommand>> = None;
        let mut active_guest = None;
        // Platform probes (uinput, clipboard tools) are costly; resolve once per connection.
        let host_caps = host_guest_capabilities();
        let mut grant_permissions = self.permission_ceiling(host_caps, true);
        let mut effective_permissions = self.permission_ceiling(host_caps, false);
        let audio_allowed = Arc::new(AtomicBool::new(false));
        let mut c2h_cipher = None;
        let mut h2c_cipher = None;
        let mut udp_peer = None;
        let mut input_order = ferryx_rd_proto::InputOrder::default();
        let mut media_receiver: Option<Receiver<MediaEvent>> = None;
        #[cfg(not(all(target_os = "linux", test)))]
        let display = self.config.display;
        #[cfg(all(target_os = "linux", not(test)))]
        let output_name = self.config.output_name.clone();
        #[cfg(all(target_os = "linux", test))]
        let prepared_input = tests::admission_input(self)?;
        let input = InputDispatch::start(move || {
            #[cfg(target_os = "macos")]
            let input = InputInjector::with_origin(
                display.logical_width as f32,
                display.logical_height as f32,
                display.desktop_x as f32,
                display.desktop_y as f32,
            );
            #[cfg(target_os = "windows")]
            let input = {
                let target = TargetDisplay {
                    x: display.desktop_x,
                    y: display.desktop_y,
                    width: display.pixel_width,
                    height: display.pixel_height,
                };
                WindowsInputInjector::new(Some(target)).map_err(SessionError::Io)?
            };
            #[cfg(all(target_os = "linux", not(test)))]
            let input = {
                let effective = reprobe_output_name(output_name.as_deref());
                let geometry =
                    probe_linux_input_geometry(effective.as_deref()).unwrap_or_else(|| {
                        crate::inject_linux::OutputGeometry::single_output(
                            display.pixel_width,
                            display.pixel_height,
                        )
                    });
                LinuxInputInjector::new(geometry).map_err(SessionError::Io)?
            };
            #[cfg(all(target_os = "linux", test))]
            let input = prepared_input;
            Ok(input)
        })?;
        // Establish the diagnostic clock before any session-local timestamps.
        let _trace = host_trace::enabled();
        let session_origin = Instant::now();
        info!(identity = negotiated_identity, peer = %tcp_peer, "TLS-PSK session established");
        let mut last_pong = Instant::now();
        let mut next_ping = Instant::now() + HEARTBEAT_INTERVAL;
        let mut stop_sender_tx = None;
        let (nack_tx, nack_rx) = mpsc::sync_channel(32);
        let mut nack_rx = Some(nack_rx);
        let mut swap_tx: Option<mpsc::Sender<(Receiver<MediaEvent>, u32, u32)>> = None;
        let mut stream_dims = (
            self.config.display.pixel_width,
            self.config.display.pixel_height,
        );
        let mut sender_thread = None;
        // Terminal status of the media pipeline. One bounded slot so a dying
        // sender thread reports its exit without ever blocking on this channel.
        let (media_status_tx, media_status_rx) = mpsc::sync_channel::<()>(1);
        let (cursor_shape_tx, cursor_shape_rx) = mpsc::sync_channel::<CursorShape>(16);
        let mut active_bitrate = self.config.bitrate;
        let idr_limiter = Arc::new(crate::idr::ForcedIdrLimiter::default());
        // Client ABR owns the encoder rate and its floor; transport bounds queues.
        let transport_bitrate = Arc::new(AtomicU64::new(u64::from(active_bitrate)));
        let mut topology = host_displays(&self.config);
        let mut selected_output = self.config.output_name.clone().or_else(|| {
            topology
                .iter()
                .find(|d| d.primary)
                .or(topology.first())
                .map(|d| d.name.clone())
        });
        let mut active_scale = 1000u16;
        let mut active_fps = self.config.frames_per_second.max(1);
        let mut active_codec = StreamCodecPreference::Auto;
        #[cfg(target_os = "macos")]
        let mut pending_mac: Option<PendingMacPipeline> = None;
        #[cfg(target_os = "macos")]
        let mut mac_recovery = MacRecoveryBudget::default();
        let mut topology_check = Instant::now() + Duration::from_secs(2);
        let mut sleep_guard = None;
        let mut clipboard_sync = false;
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        let mut image_assembly = crate::clipboard_image::Assembly::default();
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        let mut image_id = 0u32;
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        let mut next_image_poll = Instant::now();
        #[cfg(target_os = "windows")]
        let mut clipboard = Some(crate::WindowsClipboard::new());
        #[cfg(target_os = "linux")]
        let mut clipboard = crate::clipboard_linux::LinuxClipboard::new().ok();
        #[cfg(target_os = "macos")]
        let mut clipboard: Option<crate::clipboard_macos::MacClipboard> = None;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let mut next_clipboard_poll = Instant::now();
        let mut peer_has_stream_options = false;
        let mut last_sent_policy = self.config.host_policy();
        let mut input_disabled_warned = false;

        let result = (|| -> Result<(), SessionError> {
            // If authenticated and media_receiver is set, we run UDP sending in a dedicated thread to avoid TCP blocking it.
            loop {
                watch.tick(Instant::now());
                while let Ok(ack) = input.acks.try_recv() {
                    send_tcp_control(&mut stream, ControlMessage::InputAck(ack))?;
                }
                if watch.is_aborted() {
                    warn!(peer = %tcp_peer, "session watchdog aborted a stalled session");
                    break;
                }
                if stop.is_some_and(|stop| stop.load(std::sync::atomic::Ordering::Relaxed)) {
                    if state == SessionState::Authenticated {
                        send_session_end(&mut stream, peer_caps, SessionEndReason::HostStopped)?;
                    }
                    info!(peer = %tcp_peer, "host stop requested; closing connection");
                    break;
                }
                #[cfg(target_os = "macos")]
                if let Some(readiness) = pending_mac
                    .as_ref()
                    .and_then(|pending| pending.handle.readiness())
                {
                    let mut pending = pending_mac.take().expect("pending pipeline exists");
                    match readiness {
                        Err(error) => {
                            pending.handle.stop();
                            if let Some(response) = &pending.response {
                                send_tcp_control(
                                    &mut stream,
                                    ControlMessage::StreamConfigReject(StreamConfigurationReject {
                                        request_id: response.request_id,
                                        reason: StreamConfigurationErrorCode::RejectedByPeer,
                                        message: error,
                                    }),
                                )?;
                            } else {
                                return Err(SessionError::Io(io::Error::other(error)));
                            }
                        }
                        Ok(()) => {
                            if pending.response.is_none() {
                                mac_recovery.ready(Instant::now());
                            }
                            let actual_codec = pending.handle.codec();
                            pending.restart.codec = actual_codec;
                            if let Some(response) = pending.response.as_mut() {
                                response.codec = actual_codec;
                            }
                            let restart = &pending.restart;
                            let (_, empty) = mpsc::sync_channel(1);
                            let rx = std::mem::replace(&mut pending.receiver, empty);
                            if let Some(tx) = &swap_tx {
                                tx.send((rx, restart.width, restart.height))
                                    .map_err(|_| SessionError::MediaStopped)?;
                            } else {
                                media_receiver = Some(rx);
                                stream_dims = (restart.width, restart.height);
                            }
                            let old = pipeline
                                .lock()
                                .expect("pipeline slot poisoned")
                                .replace(pending.handle);
                            if let Some(mut old) = old {
                                old.stop();
                            }
                            active_bitrate = restart.bitrate;
                            transport_bitrate.store(u64::from(active_bitrate), Ordering::Release);
                            selected_output = restart.output_name.clone();
                            active_scale = restart.scale_permille;
                            active_fps = restart.fps;
                            active_codec = restart.codec;
                            if let Some(info) = restart
                                .output_name
                                .as_deref()
                                .and_then(|id| id.parse().ok())
                                .and_then(|id| ScreenCapture::display_info_for(id).ok())
                            {
                                input.dispatch(move |input| {
                                    reset_input(input);
                                    *input = InputInjector::with_origin(
                                        info.logical_width as f32,
                                        info.logical_height as f32,
                                        info.desktop_x as f32,
                                        info.desktop_y as f32,
                                    );
                                })?;
                            }
                            if let Some(response) = pending.response {
                                send_tcp_control(
                                    &mut stream,
                                    ControlMessage::StreamOptionsActive(response),
                                )?;
                            }
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                if active_codec == StreamCodecPreference::Auto {
                    let codec = pipeline
                        .lock()
                        .expect("pipeline slot poisoned")
                        .as_ref()
                        .and_then(|handle| {
                            (handle.readiness() == Some(Ok(()))).then(|| handle.codec())
                        });
                    if let Some(codec) = codec {
                        active_codec = codec;
                        if peer_has_stream_options {
                            send_tcp_control(
                                &mut stream,
                                ControlMessage::StreamOptionsActive(StreamOptionsActive {
                                    request_id: 0,
                                    width: stream_dims.0,
                                    height: stream_dims.1,
                                    fps: active_fps as u16,
                                    codec,
                                    display_index: topology
                                        .iter()
                                        .find(|display| {
                                            Some(&display.name) == selected_output.as_ref()
                                        })
                                        .map_or(0, |display| display.index),
                                    bitrate: active_bitrate,
                                }),
                            )?;
                        }
                    }
                }
                // Observe media termination before reading control, so a queued
                // orderly Disconnect wins over the sender's terminal status.
                let media_failed = media_status_rx.try_recv().is_ok();
                #[cfg(target_os = "macos")]
                mac_recovery.observe_health(Instant::now(), media_failed);
                let read = if state == SessionState::Authenticated {
                    stream.read_frame_step()
                } else {
                    stream.read_frame_until(admission_deadline)
                };
                let packet = match read {
                    Ok(packet) => packet,
                    Err(TlsPskError::Io(error))
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        None
                    }
                    Err(TlsPskError::Io(error))
                        if matches!(
                            error.kind(),
                            io::ErrorKind::UnexpectedEof
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::BrokenPipe
                        ) || error
                            .get_ref()
                            .and_then(|cause| cause.downcast_ref::<openssl::ssl::Error>())
                            .is_some_and(|ssl_err| {
                                ssl_err.code() == openssl::ssl::ErrorCode::SYSCALL
                            }) =>
                    {
                        warn!(%error, "read_frame saw EOF/reset/broken pipe, closing connection");
                        break;
                    }
                    Err(error) => {
                        warn!(%error, "read_frame failed with error, terminating connection");
                        return Err(SessionError::Tls(error));
                    }
                };
                if state == SessionState::Authenticated {
                    if let Some(packet) = &packet {
                        let (header, payload) = split_packet(packet)?;
                        if header.packet_type == PacketType::Control
                            && matches!(
                                ControlMessage::decode(payload)?,
                                ControlMessage::Disconnect | ControlMessage::StopStream
                            )
                        {
                            break;
                        }
                    }
                }
                #[cfg(target_os = "linux")]
                let audio_error = pipeline
                    .lock()
                    .expect("pipeline slot poisoned")
                    .as_ref()
                    .and_then(|handle| handle.take_audio_error());
                #[cfg(target_os = "linux")]
                if let Some(error) = audio_error {
                    send_tcp_control(
                        &mut stream,
                        ControlMessage::StreamConfigError(ferryx_rd_proto::StreamConfigurationError {
                            request_id: 0,
                            error_code: StreamConfigurationErrorCode::RejectedByPeer,
                            message: format!("audio-unavailable: {error}; retrying audio capture"),
                        }),
                    )?;
                }
                // Checked independently of UDP registration: a media pipeline that
                // died must end the session instead of serving heartbeats forever.
                if media_failed
                    || (state == SessionState::Authenticated && Instant::now() >= topology_check)
                {
                    topology_check = Instant::now() + Duration::from_secs(2);
                    let displays = host_displays(&self.config);
                    let disappeared = selected_output
                        .as_ref()
                        .is_some_and(|name| !displays.iter().any(|d| &d.name == name));
                    #[cfg(not(target_os = "macos"))]
                    if media_failed && !disappeared {
                        return Err(SessionError::MediaStopped);
                    }
                    if peer_has_stream_options && displays != topology {
                        send_tcp_control(
                            &mut stream,
                            ControlMessage::HostDisplayList(HostDisplayList {
                                displays: displays.clone(),
                            }),
                        )?;
                    }
                    topology = displays;
                    if media_failed || disappeared {
                        #[cfg(target_os = "macos")]
                        if pending_mac.is_some() {
                            continue;
                        }
                        let Some(factory) = &self.restart_factory else {
                            if media_failed {
                                return Err(SessionError::MediaStopped);
                            }
                            continue;
                        };
                        let Some(display) =
                            topology.iter().find(|d| d.primary).or(topology.first())
                        else {
                            return Err(SessionError::MediaStopped);
                        };
                        let width = ((u64::from(display.width) * u64::from(active_scale) / 1000)
                            as u32
                            & !1)
                            .max(2);
                        let height = ((u64::from(display.height) * u64::from(active_scale) / 1000)
                            as u32
                            & !1)
                            .max(2);
                        let restart = StreamRestart {
                            width,
                            height,
                            scale_permille: active_scale,
                            fps: active_fps,
                            codec: active_codec,
                            output_name: Some(display.name.clone()),
                            bitrate: active_bitrate,
                        };
                        let (tx, rx) = mpsc::sync_channel(SESSION_MEDIA_QUEUE_DEPTH);
                        let source = factory(&restart);
                        #[cfg(target_os = "macos")]
                        let handle = mac_recovery.start(source.as_ref(), tx)?;
                        #[cfg(not(target_os = "macos"))]
                        let handle = source.start_with_capabilities(tx, peer_caps)?;
                        #[cfg(target_os = "macos")]
                        {
                            if let Some(mut previous) = pending_mac.take() {
                                previous.handle.stop();
                            }
                            pending_mac = Some(PendingMacPipeline {
                                handle,
                                receiver: rx,
                                restart,
                                response: None,
                            });
                            continue;
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            active_codec = handle.codec();
                            if let Err(error) = handle.force_key_frame() {
                                warn!(%error, "failed to force key frame on restarted media pipeline");
                            }
                            if let Some(tx) = &swap_tx {
                                tx.send((rx, width, height))
                                    .map_err(|_| SessionError::MediaStopped)?;
                            } else {
                                media_receiver = Some(rx);
                                stream_dims = (width, height);
                            }
                            let old = pipeline
                                .lock()
                                .expect("pipeline slot poisoned")
                                .replace(handle);
                            if let Some(mut old) = old {
                                old.stop();
                            }
                            selected_output = Some(display.name.clone());
                            #[cfg(target_os = "linux")]
                            if let Some(geometry) =
                                probe_linux_input_geometry(selected_output.as_deref())
                            {
                                input.dispatch(move |input| {
                                    if let Err(error) = input.update_geometry(geometry) {
                                        warn!(%error, "input geometry update failed");
                                    }
                                })?;
                            }
                            #[cfg(target_os = "windows")]
                            if let Some(target) = source.output_target() {
                                input.dispatch(move |input| {
                                    input.update_target(target);
                                    if let Err(error) = input.refresh_virtual_desktop() {
                                        warn!(%error, "input desktop refresh failed");
                                    }
                                })?;
                            }
                            #[cfg(target_os = "macos")]
                            if let Some(info) = selected_output
                                .as_deref()
                                .and_then(|id| id.parse().ok())
                                .and_then(|id| ScreenCapture::display_info_for(id).ok())
                            {
                                input.dispatch(move |input| {
                                    reset_input(input);
                                    *input = InputInjector::with_origin(
                                        info.logical_width as f32,
                                        info.logical_height as f32,
                                        info.desktop_x as f32,
                                        info.desktop_y as f32,
                                    );
                                })?;
                            }
                            if peer_has_stream_options {
                                send_tcp_control(
                                    &mut stream,
                                    ControlMessage::StreamOptionsActive(StreamOptionsActive {
                                        request_id: 0,
                                        width,
                                        height,
                                        fps: active_fps as u16,
                                        codec: active_codec,
                                        display_index: display.index,
                                        bitrate: active_bitrate,
                                    }),
                                )?;
                            }
                        }
                    }
                }
                if state != SessionState::Authenticated {
                    let remaining = admission_deadline
                        .checked_duration_since(Instant::now())
                        .filter(|remaining| !remaining.is_zero())
                        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
                    stream
                        .ssl_stream()
                        .get_ref()
                        .set_write_timeout(Some(remaining))?;
                }
                if state == SessionState::Authenticated {
                    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                    if !effective_permissions.clipboard {
                        image_assembly.cancel();
                    }
                    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                    image_assembly.expire(Instant::now());
                    let mut end = None;
                    if let Some(commands) = &guest_commands {
                        while let Ok(command) = commands.try_recv() {
                            match command {
                                GuestCommand::End(reason) => {
                                    end = Some(reason);
                                    break;
                                }
                                GuestCommand::Status(status)
                                    if peer_caps.contains(Capabilities::HOST_STATUS) =>
                                {
                                    send_tcp_control(
                                        &mut stream,
                                        ControlMessage::HostStatus(status),
                                    )?
                                }
                                GuestCommand::Permissions(permissions) => {
                                    grant_permissions = permissions
                                }
                                _ => {}
                            }
                        }
                    }
                    if let PairingTrust::GuestExpiring { expires_at_unix_ms } = session_trust {
                        if unix_ms_u64() >= expires_at_unix_ms {
                            end = Some(SessionEndReason::GuestExpired);
                        }
                    }
                    if let Some(reason) = end {
                        send_session_end(&mut stream, peer_caps, reason)?;
                        break;
                    }
                    let current = guests::intersect(
                        grant_permissions,
                        self.permission_ceiling(host_caps, clipboard_sync),
                    );
                    if current != effective_permissions {
                        if (effective_permissions.keyboard && !current.keyboard)
                            || (effective_permissions.mouse && !current.mouse)
                            || (effective_permissions.gamepad && !current.gamepad)
                        {
                            let reset = InputEvent {
                                event_type: ferryx_rd_proto::InputEventType::Reset,
                                x: 0.0,
                                y: 0.0,
                                key_code: 0,
                                modifiers: ferryx_rd_proto::Modifiers::empty(),
                                scroll_dx: 0.0,
                                scroll_dy: 0.0,
                            };
                            input.dispatch(move |input| {
                                inject_input(&reset, |event| input.inject(event));
                            })?;
                        }
                        effective_permissions = current;
                        audio_allowed.store(current.audio, Ordering::SeqCst);
                        send_permissions(&mut stream, peer_caps, current)?;
                    }
                    // UDP_INPUT sessions demultiplex registered traffic and preserve
                    // unknown endpoints in the shared admission inbox; legacy sessions
                    // still stop receiving after discovery.
                    if udp_peer.is_none() {
                        self.discover_udp_peer(tcp_peer, &mut udp_peer, c2h_cipher.as_mut())?;
                    }
                    // One reader and one c2h cipher serve every registered-peer
                    // datagram type, so NACKs and input batches cannot steal each
                    // other's packets or advance the replay window separately.
                    let recovery = peer_caps.contains(Capabilities::FEC_NACK);
                    let input_lane = peer_caps.contains(Capabilities::UDP_INPUT);
                    if recovery || input_lane {
                        if let (Some(peer), Some(cipher)) = (udp_peer, c2h_cipher.as_mut()) {
                            self.receive_udp_datagrams(
                                peer,
                                cipher,
                                recovery.then_some(&nack_tx),
                                input_lane.then_some(udp_input::UdpInputSink {
                                    order: &mut input_order,
                                    input: &input,
                                    echo: peer_caps.contains(Capabilities::INPUT_ECHO),
                                    permissions: effective_permissions,
                                }),
                            )?;
                        }
                    }
                    if sender_thread.is_none() {
                        if let (Some(_), Some(peer)) = (&media_receiver, udp_peer) {
                            let mut receiver = media_receiver.take().unwrap();
                            let udp_socket = self.udp_socket.try_clone()?;
                            let mut cipher = h2c_cipher.take().ok_or(SessionError::PreAuth)?;
                            let mut pixel_width = stream_dims.0;
                            let mut pixel_height = stream_dims.1;
                            let (stx, srx) = mpsc::channel();
                            stop_sender_tx = Some(stx);
                            let (stx2, srx2) = mpsc::channel();
                            swap_tx = Some(stx2);
                            let media_status_tx = media_status_tx.clone();
                            let cursor_shape_tx = cursor_shape_tx.clone();
                            let independent_cursor = host_caps.contains(Capabilities::CURSOR_SHAPE)
                                && peer_caps.contains(Capabilities::CURSOR_SHAPE);
                            let audio_allowed = audio_allowed.clone();
                            #[cfg(test)]
                            let media_source = self.media_source.clone();
                            let sender_pipeline = pipeline.clone();
                            let sender_bitrate = active_bitrate;
                            let idr_limiter = Arc::clone(&idr_limiter);
                            let transport_bitrate = transport_bitrate.clone();
                            let recovery_enabled = peer_caps.contains(Capabilities::FEC_NACK);
                            let nack_rx = nack_rx.take().expect("single sender");
                            let input_echo = peer_caps
                                .contains(Capabilities::INPUT_ECHO)
                                .then(|| input.applied.clone());
                            let handle = thread::spawn(move || {
                                let mut sender = UdpSender {
                                    input_echo,
                                    audio_source_time_enabled: peer_caps
                                        .contains(Capabilities::AUDIO_SOURCE_TIME),
                                    audio_session_origin: Some(session_origin),
                                    audio_opus_enabled: peer_caps
                                        .contains(Capabilities::AUDIO_OPUS),
                                    incremental: true,
                                    idr_limiter,
                                    bitrate: sender_bitrate,
                                    send_capacity: rustix::net::sockopt::socket_send_buffer_size(
                                        &udp_socket,
                                    )
                                    .unwrap_or_else(|error| {
                                        warn!(%error, "cannot read sender capacity");
                                        32 * 1024
                                    }),
                                    ..Default::default()
                                };
                                let mut queued_video = std::collections::VecDeque::new();
                                let mut pending_cursor_shape = None;
                                sender.trace = host_trace::enabled().cloned();
                                sender.trace_session =
                                    sender.trace.as_ref().map_or(0, |t| t.time(session_origin));
                                info!(%peer, "Starting UDP sender thread");
                                loop {
                                    sender.bitrate =
                                        transport_bitrate.load(Ordering::Acquire) as u32;
                                    sender.recovery.enabled = recovery_enabled;
                                    // Bound feedback processing too: a concurrent producer
                                    // must not keep try_iter alive and starve audio dispatch.
                                    for nack in nack_rx.try_iter().take(32) {
                                        sender.recovery.enqueue(nack, Instant::now());
                                    }
                                    if let Err(error) = sender.send_retransmit_burst(
                                        &udp_socket,
                                        peer,
                                        &mut cipher,
                                        Instant::now(),
                                    ) {
                                        warn!(%error, "NACK retransmission failed");
                                    }
                                    if let Some(shape) = pending_cursor_shape.take() {
                                        match cursor_shape_tx.try_send(shape) {
                                            Ok(()) => {}
                                            Err(mpsc::TrySendError::Full(shape)) => {
                                                pending_cursor_shape = Some(shape)
                                            }
                                            Err(mpsc::TrySendError::Disconnected(_)) => break,
                                        }
                                    }
                                    while let Ok((r, w, h)) = srx2.try_recv() {
                                        receiver = r;
                                        queued_video.clear();
                                        sender.pending = None;
                                        sender.needs_keyframe = true;
                                        pixel_width = w;
                                        pixel_height = h;
                                    }
                                    if !matches!(srx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                                        info!("UDP sender received stop signal");
                                        break;
                                    }
                                    #[cfg(target_os = "macos")]
                                    {
                                        if !audio_allowed.load(Ordering::SeqCst) {
                                            sender.mac_audio = None;
                                        }
                                        if let Err(error) =
                                            sender.send_mac_audio(&udp_socket, peer, &mut cipher)
                                        {
                                            warn!(%error, "Failed to send macOS audio over UDP");
                                        }
                                    }
                                    if sender
                                        .pending
                                        .as_ref()
                                        .is_some_and(|pending| Instant::now() >= pending.next_at)
                                    {
                                        if let Err(error) =
                                            sender.send_video_burst(&udp_socket, peer, &mut cipher)
                                        {
                                            warn!(%error, "video burst dropped; recovering reference chain");
                                            queued_video.clear();
                                            sender.invalidate_reference(&sender_pipeline);
                                        }
                                    }
                                    sender.request_recovery(&sender_pipeline, Instant::now());
                                    let wait = sender.pending.as_ref().map_or(
                                        Duration::from_millis(5),
                                        |pending| {
                                            pending
                                                .next_at
                                                .saturating_duration_since(Instant::now())
                                        },
                                    );
                                    let event =
                                        if sender.pending.is_none() && !queued_video.is_empty() {
                                            Ok(MediaEvent::Video(
                                                queued_video.pop_front().expect("queued frame"),
                                            ))
                                        } else {
                                            receiver.recv_timeout(wait)
                                        };
                                    match event {
                                        #[cfg(target_os = "macos")]
                                        Ok(MediaEvent::MacReady(wakeup)) => {
                                            wakeup.notify();
                                            sender.mac_wakeup = Some(wakeup);
                                        }

                                        Ok(MediaEvent::Video(frame)) => {
                                            #[cfg(target_os = "macos")]
                                            {
                                                if let Some(wakeup) = &sender.mac_wakeup {
                                                    wakeup.notify();
                                                }
                                                if !audio_allowed.load(Ordering::SeqCst) {
                                                    sender.mac_audio = None;
                                                }
                                            }

                                            if sender.pending.is_some() {
                                                if queued_video.len() == 16 {
                                                    queued_video.clear();
                                                    sender.invalidate_reference(&sender_pipeline);
                                                } else {
                                                    queued_video.push_back(frame);
                                                    continue;
                                                }
                                            }
                                            if video_expired(&frame, Instant::now()) {
                                                sender.invalidate_reference(&sender_pipeline);
                                                continue;
                                            }
                                            if sender.needs_keyframe && !frame.is_key_frame {
                                                continue;
                                            }
                                            let size = frame.data.len();
                                            let is_key = frame.is_key_frame;
                                            if let Err(err) = sender.send_frame(
                                                &udp_socket,
                                                peer,
                                                &mut cipher,
                                                pixel_width,
                                                pixel_height,
                                                frame,
                                                session_origin,
                                            ) {
                                                warn!(%err, "Failed to send video frame over UDP");
                                                sender.invalidate_reference(&sender_pipeline);
                                            } else {
                                                debug!(size, is_key, %peer, "Video frame admitted to deadline scheduler");
                                            }
                                        }
                                        #[cfg(target_os = "macos")]
                                        Ok(MediaEvent::MacAudio(slot)) => {
                                            if audio_allowed.load(Ordering::SeqCst) {
                                                sender.mac_audio = Some(slot);
                                                if let Err(error) = sender.send_mac_audio(
                                                    &udp_socket,
                                                    peer,
                                                    &mut cipher,
                                                ) {
                                                    warn!(%error, "Failed to send macOS audio over UDP");
                                                }
                                            } else {
                                                sender.mac_audio = None;
                                                slot.take(Instant::now());
                                            }
                                        }
                                        #[cfg(target_os = "windows")]
                                        Ok(MediaEvent::TimedAudio { pcm, captured_at }) => {
                                            if !audio_allowed.load(Ordering::SeqCst) {
                                                continue;
                                            }
                                            sender.audio_capture_at = Some(captured_at);
                                            if let Err(error) = sender.send_audio(
                                                &udp_socket,
                                                peer,
                                                &mut cipher,
                                                &pcm,
                                            ) {
                                                warn!(%error, "audio datagram send failed");
                                            }
                                        }
                                        Ok(MediaEvent::Audio(bytes)) => {
                                            if !audio_allowed.load(Ordering::SeqCst) {
                                                continue;
                                            }
                                            if let Err(error) = sender.send_audio(
                                                &udp_socket,
                                                peer,
                                                &mut cipher,
                                                &bytes,
                                            ) {
                                                warn!(%error, "audio datagram send failed");
                                            }
                                        }
                                        #[cfg(target_os = "linux")]
                                        Ok(MediaEvent::NativeAudio(slot)) => {
                                            if !audio_allowed.load(Ordering::SeqCst) {
                                                continue;
                                            }
                                            if let Some(block) = slot.take_fresh(Instant::now()) {
                                                sender.audio_capture_at = Some(block.captured_at);
                                                if let Err(error) = sender.send_audio(
                                                    &udp_socket,
                                                    peer,
                                                    &mut cipher,
                                                    &block.pcm,
                                                ) {
                                                    warn!(%error, "Failed to send native audio over UDP");
                                                }
                                            }
                                        }
                                        Ok(MediaEvent::Cursor(cursor, shape_id, shape)) => {
                                            #[cfg(target_os = "macos")]
                                            if let Some(wakeup) = &sender.mac_wakeup {
                                                wakeup.notify();
                                            }
                                            if independent_cursor {
                                                if let Some(shape) = shape {
                                                    pending_cursor_shape = Some(shape);
                                                    match cursor_shape_tx.try_send(
                                                        pending_cursor_shape
                                                            .take()
                                                            .expect("pending shape"),
                                                    ) {
                                                        Ok(()) => {}
                                                        Err(mpsc::TrySendError::Full(shape)) => {
                                                            pending_cursor_shape = Some(shape)
                                                        }
                                                        Err(mpsc::TrySendError::Disconnected(
                                                            _,
                                                        )) => break,
                                                    }
                                                }
                                            }
                                            let _ = sender.send_cursor(
                                                &udp_socket,
                                                peer,
                                                &mut cipher,
                                                &cursor,
                                                independent_cursor.then_some(shape_id.unwrap_or(0)),
                                            );
                                        }
                                        Ok(MediaEvent::Error(err)) => {
                                            // The replaced pipeline may report its own stop as
                                            // an error; switch to the new one instead of ending.
                                            if let Ok((r, w, h)) = srx2.try_recv() {
                                                warn!(%err, "replaced media pipeline reported an error");
                                                receiver = r;
                                                pixel_width = w;
                                                pixel_height = h;
                                                continue;
                                            }
                                            warn!(%err, "Media event error in sender thread");
                                            let _ = media_status_tx.try_send(());
                                            // Teardown drops swap_tx, waking this receive;
                                            // hotplug recovery supplies the replacement.
                                            match srx2.recv() {
                                                Ok((r, w, h)) => {
                                                    receiver = r;
                                                    pixel_width = w;
                                                    pixel_height = h;
                                                }
                                                Err(_) => break,
                                            }
                                        }
                                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                                            // A restart hands over the new receiver before it
                                            // stops the old pipeline, so a pending swap means
                                            // this disconnect is the replaced pipeline ending.
                                            if let Ok((r, w, h)) = srx2.try_recv() {
                                                receiver = r;
                                                pixel_width = w;
                                                pixel_height = h;
                                                continue;
                                            }
                                            warn!("Media receiver disconnected, exiting sender thread");
                                            let _ = media_status_tx.try_send(());
                                            match srx2.recv() {
                                                Ok((r, w, h)) => {
                                                    receiver = r;
                                                    pixel_width = w;
                                                    pixel_height = h;
                                                }
                                                Err(_) => break,
                                            }
                                        }
                                    }
                                }
                                drop(receiver);
                                #[cfg(test)]
                                media_source.sender_exited();
                            });
                            sender_thread = Some(handle);
                        }
                    }
                    let now = Instant::now();
                    while let Ok(shape) = cursor_shape_rx.try_recv() {
                        if host_caps.contains(Capabilities::CURSOR_SHAPE)
                            && peer_caps.contains(Capabilities::CURSOR_SHAPE)
                        {
                            send_tcp_control(&mut stream, ControlMessage::CursorShape(shape))?;
                        }
                    }
                    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                    if clipboard_sync
                        && effective_permissions.clipboard
                        && peer_caps.contains(Capabilities::CLIPBOARD_IMAGE)
                        && host_caps.contains(Capabilities::CLIPBOARD_IMAGE)
                        && now >= next_image_poll
                    {
                        next_image_poll = now + Duration::from_millis(500);
                        if let Some(clipboard) = clipboard.as_mut() {
                            match clipboard.poll_image() {
                                Ok(Some(bytes)) => {
                                    image_id = image_id
                                        .checked_add(1)
                                        .ok_or(SessionError::MediaStopped)?;
                                    for (index, chunk) in bytes.chunks(65536).enumerate() {
                                        send_tcp_control(
                                            &mut stream,
                                            ControlMessage::ClipboardImageChunk(
                                                ferryx_rd_proto::ClipboardImageChunk {
                                                    transfer_id: image_id,
                                                    total_bytes: bytes.len() as u32,
                                                    offset: (index * 65536) as u32,
                                                    png_bytes: chunk.to_vec(),
                                                },
                                            ),
                                        )?;
                                    }
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    tracing::debug!(%error, "clipboard image poll unavailable")
                                }
                            }
                        }
                    }
                    if peer_has_stream_options
                        && !peer_caps.contains(Capabilities::GUEST_PERMISSIONS)
                    {
                        let current_policy = HostPolicy {
                            allow_input: effective_permissions.keyboard
                                && effective_permissions.mouse,
                            allow_clipboard: effective_permissions.clipboard,
                        };
                        if current_policy != last_sent_policy {
                            if let Err(error) = send_tcp_control(
                                &mut stream,
                                ControlMessage::HostPolicy(current_policy),
                            ) {
                                warn!(%error, "failed to send updated host policy");
                            } else {
                                last_sent_policy = current_policy;
                            }
                        }
                    }
                    if now >= next_ping {
                        send_tcp_control(&mut stream, ControlMessage::Ping)?;
                        next_ping = now + HEARTBEAT_INTERVAL;
                    }
                    if now.duration_since(last_pong) >= HEARTBEAT_TIMEOUT {
                        warn!(peer = %tcp_peer, "heartbeat timeout");
                        break;
                    }
                    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                    if clipboard_sync && effective_permissions.clipboard {
                        // Linux reads the pasteboard through a subprocess and
                        // macOS through an IPC round trip to pboard, so both are
                        // gated to the protocol's 500 ms cadence; the Win32
                        // poll is a single syscall and runs every iteration.
                        #[cfg(any(target_os = "linux", target_os = "macos"))]
                        let clipboard_due = now >= next_clipboard_poll;
                        #[cfg(target_os = "linux")]
                        if clipboard_due {
                            next_clipboard_poll =
                                now + crate::clipboard_linux::DEFAULT_POLL_INTERVAL;
                        }
                        #[cfg(target_os = "macos")]
                        if clipboard_due {
                            next_clipboard_poll = now + crate::clipboard_macos::POLL_INTERVAL;
                        }
                        #[cfg(target_os = "windows")]
                        let clipboard_due = true;
                        if clipboard_due {
                            if let Some(clipboard) = clipboard.as_mut() {
                                let polled = {
                                    #[cfg(any(target_os = "windows", target_os = "macos"))]
                                    {
                                        clipboard.poll()
                                    }
                                    #[cfg(target_os = "linux")]
                                    {
                                        clipboard.poll(now)
                                    }
                                };
                                match polled {
                                    Ok(Some(text)) => {
                                        debug!(bytes = text.len(), "clipboard change detected");
                                        send_tcp_control(
                                            &mut stream,
                                            ControlMessage::ClipboardSyncUpdate(
                                                ClipboardSyncUpdate {
                                                    request_id: 0,
                                                    direction: ClipboardSyncDirection::HostToClient,
                                                    origin: ClipboardSyncOrigin::LocalPasteboard,
                                                    text,
                                                },
                                            ),
                                        )?;
                                    }
                                    Ok(None) => {}
                                    Err(error) => {
                                        debug!(%error, "clipboard poll failed");
                                    }
                                }
                            }
                        }
                    }
                }

                let Some(packet) = packet else {
                    continue;
                };
                let (header, payload) = split_packet(&packet)?;
                if !state.allows(header.packet_type) {
                    debug!(?state, ?header.packet_type, "refusing packet for current state");
                    if state == SessionState::Authenticated
                        && header.packet_type == PacketType::Handshake
                    {
                        return Err(SessionError::AlreadyAuthenticated);
                    }
                    continue;
                }

                match header.packet_type {
                    PacketType::PairingRequest => {
                        if negotiated_identity != BOOTSTRAP_IDENTITY || !self.is_pairing_active() {
                            send_pairing_reject(&mut stream, PairingRejectReason::PairingDisabled)?;
                            continue;
                        }
                        let request = PairingRequest::decode(payload)?;
                        let (response_tx, response_rx) = mpsc::sync_channel(1);
                        let request_id = Uuid::new_v4().to_string();
                        self.guests.request(
                            PendingRequest {
                                id: request_id.clone(),
                                name: request.name.clone(),
                                expires_at: admission_deadline.min(
                                    self.pairing_deadline.ok_or(SessionError::ConsentTimeout)?,
                                ),
                            },
                            response_tx.clone(),
                        );
                        if let Some(consent_sender) = &self.config.consent_sender {
                            consent_sender
                                .send(ConsentPrompt {
                                    client_name: request.name.clone(),
                                    response: response_tx,
                                })
                                .map_err(|_| {
                                    self.guests.finish_request(&request_id);
                                    SessionError::ConsentUnavailable
                                })?;
                        }
                        let approval = response_rx.recv_timeout(
                            admission_deadline
                                .min(self.pairing_deadline.ok_or(SessionError::ConsentTimeout)?)
                                .saturating_duration_since(Instant::now()),
                        );
                        self.guests.finish_request(&request_id);
                        let approved = approval.map_err(|_| SessionError::ConsentTimeout)?;
                        if Instant::now() >= admission_deadline || !self.is_pairing_active() {
                            return Err(SessionError::ConsentTimeout);
                        }
                        let Some(trust) = approved else {
                            send_pairing_reject(&mut stream, PairingRejectReason::DeniedByHost)?;
                            break;
                        };
                        trust.encode()?;
                        if trust != PairingTrust::GuestOnce
                            && !guests::valid_trust(trust, unix_ms_u64())
                        {
                            return Err(SessionError::ConsentTimeout);
                        }
                        session_trust = trust;
                        let record = PairingRecord {
                            id: Uuid::new_v4().to_string().to_uppercase(),
                            name: request.name,
                            key: random_key(),
                            added_at_unix_ms: unix_ms_u64(),
                        };
                        let grant = PairingGrant {
                            pairing_id: record.id.clone(),
                            host_name: self.config.host_name.clone(),
                            key: record.key,
                        };
                        send_tcp_packet(&mut stream, PacketType::PairingGrant, &grant.encode()?)?;
                        granted_pairing_id = Some(record.id.clone());
                        provisional = Some(record);
                        state = SessionState::PairingGranted;
                    }
                    PacketType::Handshake => {
                        if state == SessionState::Authenticated {
                            return Err(SessionError::AlreadyAuthenticated);
                        }
                        let handshake = Handshake::decode(payload)?;
                        let fresh = provisional.is_some();
                        let record = if let Some(record) = &provisional {
                            record.clone()
                        } else {
                            self.config
                                .pairing_store
                                .load(&handshake.pairing_id)?
                                .ok_or(SessionError::UnknownPairing)?
                        };
                        if !fresh {
                            let (trust, permissions) =
                                self.config.pairing_store.authorization(&record.id)?;
                            if !guests::valid_trust(trust, unix_ms_u64()) {
                                return Err(SessionError::UnknownPairing);
                            }
                            session_trust = trust;
                            if let Some(permissions) = permissions {
                                grant_permissions = permissions;
                            }
                        }
                        if let Some(identity_pairing_id) =
                            negotiated_identity.strip_prefix(PAIRING_IDENTITY_PREFIX)
                        {
                            if identity_pairing_id != handshake.pairing_id {
                                return Err(SessionError::IdentityMismatch);
                            }
                        } else if negotiated_identity == BOOTSTRAP_IDENTITY {
                            let Some(expected_id) = &granted_pairing_id else {
                                return Err(SessionError::PreAuth);
                            };
                            if *expected_id != handshake.pairing_id {
                                return Err(SessionError::IdentityMismatch);
                            }
                        } else {
                            return Err(SessionError::IdentityMismatch);
                        }
                        if handshake.version != PROTOCOL_VERSION {
                            return Err(SessionError::Codec(
                                ferryx_rd_proto::CodecError::UnsupportedVersion(handshake.version),
                            ));
                        }
                        clipboard_sync = host_caps.contains(Capabilities::TEXT_CLIPBOARD_SYNC)
                            && handshake
                                .capabilities
                                .contains(Capabilities::TEXT_CLIPBOARD_SYNC);
                        // AppKit's first pasteboard call is a slow pboard round
                        // trip; open it only for an authenticated peer that
                        // negotiated clipboard sync, never inside admission.
                        #[cfg(target_os = "macos")]
                        if clipboard_sync && clipboard.is_none() {
                            clipboard = Some(crate::clipboard_macos::MacClipboard::new());
                        }
                        if !handshake
                            .capabilities
                            .contains(Capabilities::AUTHENTICATED_UDP_REGISTRATION)
                        {
                            return Err(SessionError::MissingAuthenticatedRegistration);
                        }
                        debug!(
                            clipboard_sync,
                            bits = handshake.capabilities.bits(),
                            "capabilities negotiated"
                        );
                        c2h_cipher = Some(DatagramCipher::derive(
                            &record.key,
                            &handshake.session_salt,
                            Direction::ClientToHost,
                        )?);
                        h2c_cipher = Some(DatagramCipher::derive(
                            &record.key,
                            &handshake.session_salt,
                            Direction::HostToClient,
                        )?);
                        peer_caps = handshake.capabilities;
                        #[cfg(target_os = "macos")]
                        {
                            mac_recovery.peer_capabilities = peer_caps;
                        }
                        stream
                            .ssl_stream()
                            .get_ref()
                            .set_write_timeout(Some(Duration::from_secs(5)))?;
                        peer_has_stream_options = handshake
                            .capabilities
                            .contains(Capabilities::STREAM_OPTIONS);
                        let ack = Handshake {
                            name: self.config.host_name.clone(),
                            width: self.config.display.logical_width.min(u16::MAX as u32) as u16,
                            height: self.config.display.logical_height.min(u16::MAX as u32) as u16,
                            scale: self.config.display.scale_factor(),
                            version: PROTOCOL_VERSION,
                            capabilities: Capabilities::STREAM_CONFIGURATION
                                | Capabilities::AUTHENTICATED_UDP_REGISTRATION
                                | Capabilities::STREAM_OPTIONS
                                | host_caps,
                            pairing_id: String::new(),
                            session_salt: [0_u8; 16],
                        };
                        if !peer_caps.contains(Capabilities::PAIRING_TRUST) {
                            let (media_tx, media_rx) =
                                mpsc::sync_channel(SESSION_MEDIA_QUEUE_DEPTH);
                            let started = self
                                .media_source
                                .start_with_capabilities(media_tx, peer_caps)?;
                            active_codec = started.codec();
                            sleep_guard = Some(crate::sleep_inhibit::StreamingGuard::acquire());
                            *pipeline.lock().expect("pipeline slot poisoned") = Some(started);
                            media_receiver = Some(media_rx);
                        }
                        send_tcp_packet(&mut stream, PacketType::HandshakeAck, &ack.encode()?)?;
                        if peer_caps.contains(Capabilities::PAIRING_TRUST) {
                            if fresh {
                                let packet = stream
                                    .read_frame_until(admission_deadline)?
                                    .ok_or(SessionError::ConsentTimeout)?;
                                let (header, payload) = split_packet(&packet)?;
                                if header.packet_type != PacketType::Control
                                    || !matches!(
                                        ControlMessage::decode(payload)?,
                                        ControlMessage::PairingTrustRequest(_)
                                    )
                                {
                                    return Err(SessionError::PreAuth);
                                }
                            }
                            send_tcp_control(
                                &mut stream,
                                ControlMessage::PairingTrustGrant(PairingTrustGrant {
                                    trust: session_trust,
                                }),
                            )?;
                        }
                        if fresh && session_trust != PairingTrust::GuestOnce {
                            if !guests::valid_trust(session_trust, unix_ms_u64()) {
                                send_session_end(
                                    &mut stream,
                                    peer_caps,
                                    SessionEndReason::GuestExpired,
                                )?;
                                break;
                            }
                            self.config.pairing_store.set_authorization(
                                &record.id,
                                session_trust,
                                Some(grant_permissions),
                            )?;
                            self.config.pairing_store.save(record.clone())?;
                        }
                        provisional = None;
                        effective_permissions = guests::intersect(
                            grant_permissions,
                            self.permission_ceiling(host_caps, clipboard_sync),
                        );
                        send_permissions(&mut stream, peer_caps, effective_permissions)?;
                        audio_allowed.store(effective_permissions.audio, Ordering::SeqCst);
                        let guest = GuestSession {
                            id: Uuid::new_v4().to_string(),
                            pairing_id: record.id.clone(),
                            name: record.name.clone(),
                            peer: tcp_peer,
                            trust: session_trust,
                            connected_at: Instant::now(),
                        };
                        let (guard, commands) = self.guests.join(guest.clone());
                        guest_guard = Some(guard);
                        guest_commands = Some(commands);
                        active_guest = Some(guest);
                        state = SessionState::Authenticated;
                        if peer_caps.contains(Capabilities::PAIRING_TRUST) {
                            let (media_tx, media_rx) =
                                mpsc::sync_channel(SESSION_MEDIA_QUEUE_DEPTH);
                            let started = self
                                .media_source
                                .start_with_capabilities(media_tx, peer_caps)?;
                            active_codec = started.codec();
                            sleep_guard = Some(crate::sleep_inhibit::StreamingGuard::acquire());
                            *pipeline.lock().expect("pipeline slot poisoned") = Some(started);
                            media_receiver = Some(media_rx);
                        }
                        if peer_has_stream_options {
                            last_sent_policy = HostPolicy {
                                allow_input: effective_permissions.keyboard
                                    && effective_permissions.mouse,
                                allow_clipboard: effective_permissions.clipboard,
                            };
                            let displays = host_displays(&self.config);
                            send_tcp_control(
                                &mut stream,
                                ControlMessage::HostDisplayList(HostDisplayList { displays }),
                            )?;
                            if active_codec != StreamCodecPreference::Auto {
                                send_tcp_control(
                                    &mut stream,
                                    ControlMessage::StreamOptionsActive(StreamOptionsActive {
                                        request_id: 0,
                                        width: stream_dims.0,
                                        height: stream_dims.1,
                                        fps: active_fps as u16,
                                        codec: active_codec,
                                        display_index: 0,
                                        bitrate: active_bitrate,
                                    }),
                                )?;
                            }
                        }
                        last_pong = Instant::now();
                        next_ping = Instant::now() + HEARTBEAT_INTERVAL;
                        info!(
                            client = handshake.name,
                            "v3 handshake authenticated; UDP ciphers armed"
                        );
                    }
                    PacketType::InputBatch => {
                        if !peer_caps.contains(Capabilities::UDP_INPUT) {
                            return Err(SessionError::PreAuth);
                        }
                        let batch = ferryx_rd_proto::InputBatch::decode(payload)?;
                        let mut events = Vec::new();
                        for mut event in input_order.accept(&batch, true) {
                            if !effective_permissions.mouse {
                                continue;
                            }
                            if !effective_permissions.keyboard {
                                event.modifiers = ferryx_rd_proto::Modifiers::empty();
                            }
                            events.push(event);
                        }
                        if peer_caps.contains(Capabilities::INPUT_ECHO) {
                            input.inject_batch(header.sequence, batch.sequence, events)?;
                        } else {
                            for event in events {
                                input.inject(header.sequence, event)?;
                            }
                        }
                    }
                    PacketType::InputEvent => {
                        if state != SessionState::Authenticated {
                            continue;
                        }
                        let mut echo_sequence = None;
                        let mut echo_events = Vec::new();
                        let mut event = if peer_caps.contains(Capabilities::UDP_INPUT) {
                            if payload.len() < InputEvent::SIZE {
                                return Err(SessionError::PreAuth);
                            }
                            let event = InputEvent::decode(&payload[..InputEvent::SIZE])?;
                            let batch =
                                ferryx_rd_proto::InputBatch::decode(&payload[InputEvent::SIZE..])?;
                            echo_sequence = Some(batch.sequence);
                            let mut motions = Vec::new();
                            for mut motion in input_order.accept(&batch, true) {
                                if !effective_permissions.mouse {
                                    continue;
                                }
                                if !effective_permissions.keyboard {
                                    motion.modifiers = ferryx_rd_proto::Modifiers::empty();
                                }
                                motions.push(motion);
                            }
                            if peer_caps.contains(Capabilities::INPUT_ECHO) {
                                echo_events = motions;
                            } else {
                                for motion in motions {
                                    input.inject(header.sequence, motion)?;
                                }
                            }
                            event
                        } else {
                            InputEvent::decode(payload)?
                        };
                        if !effective_permissions.keyboard {
                            event.modifiers = ferryx_rd_proto::Modifiers::empty();
                        }
                        let gamepad = matches!(
                            event.event_type,
                            ferryx_rd_proto::InputEventType::GamepadAxis
                                | ferryx_rd_proto::InputEventType::GamepadButtonDown
                                | ferryx_rd_proto::InputEventType::GamepadButtonUp
                        );
                        if !guests::allows(effective_permissions, event.event_type)
                            || (gamepad && !peer_caps.contains(Capabilities::GAMEPAD))
                        {
                            if !input_disabled_warned {
                                input_disabled_warned = true;
                                warn!("input disabled by host policy");
                                crate::host_log("input disabled by host policy");
                            }
                            let ack = InputAckMessage {
                                sequence: header.sequence,
                                success: false,
                                error_code: 2,
                            };
                            if let Err(error) =
                                send_tcp_control(&mut stream, ControlMessage::InputAck(ack))
                            {
                                warn!(%error, "failed to send input ACK");
                            }
                            if !echo_events.is_empty() {
                                // Apply permitted motion, but never echo the rejected discrete input.
                                for motion in echo_events {
                                    input.inject(header.sequence, motion)?;
                                }
                            }
                            continue;
                        }
                        if peer_caps.contains(Capabilities::INPUT_ECHO) {
                            if let Some(sequence) = echo_sequence {
                                echo_events.push(event);
                                input.inject_batch(header.sequence, sequence, echo_events)?;
                            } else {
                                input.inject(header.sequence, event)?;
                            }
                        } else {
                            input.inject(header.sequence, event)?;
                        }
                    }
                    PacketType::Control => {
                        if state != SessionState::Authenticated {
                            continue;
                        }
                        let control = ControlMessage::decode(payload)?;
                        if !control.message_type().supported_by(
                            host_caps
                                | Capabilities::STREAM_OPTIONS
                                | Capabilities::STREAM_CONFIGURATION,
                            peer_caps,
                        ) {
                            return Err(SessionError::PreAuth);
                        }
                        match control {
                            ControlMessage::ClockPing(mut probe) => {
                                probe.host_receive_us =
                                    monotonic_us(session_origin, Instant::now());
                                probe.host_send_us = monotonic_us(session_origin, Instant::now());
                                send_tcp_control(&mut stream, ControlMessage::ClockPong(probe))?
                            }
                            ControlMessage::RttPing(probe) => {
                                send_tcp_control(&mut stream, ControlMessage::RttPong(probe))?
                            }
                            ControlMessage::SecureAttentionRequest(request) => {
                                let result = self.guests.secure_attention(
                                    active_guest.as_ref().ok_or(SessionError::PreAuth)?,
                                );
                                send_tcp_control(
                                    &mut stream,
                                    ControlMessage::SecureAttentionResponse(
                                        SecureAttentionResponse {
                                            request_id: request.request_id,
                                            result,
                                        },
                                    ),
                                )?;
                            }
                            ControlMessage::PairingTrustRequest(_)
                            | ControlMessage::PairingTrustGrant(_)
                            | ControlMessage::GuestPermissions(_)
                            | ControlMessage::HostStatus(_)
                            | ControlMessage::SessionEnded(_)
                            | ControlMessage::ClockPong(_)
                            | ControlMessage::RttPong(_)
                            | ControlMessage::CursorShape(_)
                            | ControlMessage::SecureAttentionResponse(_) => {
                                return Err(SessionError::PreAuth)
                            }
                            ControlMessage::ClipboardImageChunk(chunk) => {
                                #[cfg(any(
                                    target_os = "windows",
                                    target_os = "linux",
                                    target_os = "macos"
                                ))]
                                if clipboard_sync
                                    && effective_permissions.clipboard
                                    && peer_caps.contains(Capabilities::CLIPBOARD_IMAGE)
                                    && host_caps.contains(Capabilities::CLIPBOARD_IMAGE)
                                {
                                    match image_assembly.push(chunk, Instant::now()) {
                                        Ok(Some(bytes)) => {
                                            if let Some(clipboard) = clipboard.as_mut() {
                                                if let Err(error) =
                                                    clipboard.apply_remote_image(&bytes)
                                                {
                                                    warn!(%error, "remote image clipboard failed");
                                                }
                                            }
                                        }
                                        Ok(None) => {}
                                        Err(error) => {
                                            warn!(%error, "clipboard image transfer rejected")
                                        }
                                    }
                                } else {
                                    image_assembly.cancel();
                                }
                                #[cfg(not(any(
                                    target_os = "windows",
                                    target_os = "linux",
                                    target_os = "macos"
                                )))]
                                let _ = chunk;
                            }
                            ControlMessage::RequestKeyFrame => {
                                request_forced_idr(&idr_limiter, pipeline, Instant::now())?;
                            }
                            ControlMessage::BitrateAdjust(BitrateAdjust { target_bitrate })
                                if target_bitrate > 0 =>
                            {
                                let slot = pipeline.lock().expect("pipeline slot poisoned");
                                if let Some(media) = slot.as_ref() {
                                    if let Err(error) = media.update_bitrate(target_bitrate as u32)
                                    {
                                        // A rejected quality change must not end the
                                        // session; the stream stays at its old bitrate.
                                        warn!(%error, target_bitrate, "bitrate adjust failed");
                                    } else {
                                        active_bitrate = target_bitrate as u32;
                                        transport_bitrate
                                            .store(u64::from(active_bitrate), Ordering::Release);
                                    }
                                }
                            }
                            ControlMessage::StreamConfigRequest(req) => {
                                let reject_reason = if req.desired.width == 0
                                    || req.desired.height == 0
                                    || req.desired.width > 7680
                                    || req.desired.height > 4320
                                {
                                    Some((
                                        StreamConfigurationErrorCode::UnsupportedDimensions,
                                        "unsupported dimensions",
                                    ))
                                } else if req.desired.frames_per_second == 0
                                    || req.desired.frames_per_second > 240
                                {
                                    Some((
                                        StreamConfigurationErrorCode::UnsupportedFps,
                                        "unsupported fps",
                                    ))
                                } else if req.desired.bitrate < 100_000
                                    || req.desired.bitrate > 300_000_000
                                {
                                    Some((
                                        StreamConfigurationErrorCode::UnsupportedBitrate,
                                        "unsupported bitrate",
                                    ))
                                } else {
                                    None
                                };
                                match reject_reason {
                                    Some((reason, message)) => {
                                        send_tcp_control(
                                            &mut stream,
                                            ControlMessage::StreamConfigReject(
                                                StreamConfigurationReject {
                                                    request_id: req.request_id,
                                                    reason,
                                                    message: message.to_owned(),
                                                },
                                            ),
                                        )?;
                                    }
                                    None => {
                                        // A submission that failed must never be reported
                                        // back to the client as the active configuration.
                                        let applied = {
                                            let slot =
                                                pipeline.lock().expect("pipeline slot poisoned");
                                            match slot.as_ref() {
                                                Some(media) => media
                                                    .update_bitrate(req.desired.bitrate)
                                                    .map(|()| req.desired.bitrate),
                                                None => Ok(active_bitrate),
                                            }
                                        };
                                        match applied {
                                            Ok(bitrate) => {
                                                active_bitrate = bitrate;
                                                transport_bitrate.store(
                                                    u64::from(active_bitrate),
                                                    Ordering::Release,
                                                );
                                                let active = StreamConfiguration {
                                                    width: self.config.display.pixel_width,
                                                    height: self.config.display.pixel_height,
                                                    bitrate: active_bitrate,
                                                    frames_per_second: self.config.frames_per_second
                                                        as u16,
                                                };
                                                send_tcp_control(
                                                    &mut stream,
                                                    ControlMessage::StreamConfigResponse(
                                                        StreamConfigurationResponse {
                                                            request_id: req.request_id,
                                                            active,
                                                        },
                                                    ),
                                                )?;
                                            }
                                            Err(error) => {
                                                warn!(%error, "stream configuration bitrate was not applied");
                                                send_tcp_control(
                                                    &mut stream,
                                                    ControlMessage::StreamConfigReject(
                                                        StreamConfigurationReject {
                                                            request_id: req.request_id,
                                                            reason: StreamConfigurationErrorCode::UnsupportedBitrate,
                                                            message: "bitrate update was not applied"
                                                                .to_owned(),
                                                        },
                                                    ),
                                                )?;
                                            }
                                        }
                                    }
                                }
                            }
                            ControlMessage::StreamOptionsRequest(request) => {
                                let displays = host_displays(&self.config);
                                match validate_stream_options(&request, &displays) {
                                    Err((reason, message)) => {
                                        send_tcp_control(
                                            &mut stream,
                                            ControlMessage::StreamConfigReject(
                                                StreamConfigurationReject {
                                                    request_id: request.request_id,
                                                    reason,
                                                    message,
                                                },
                                            ),
                                        )?;
                                        continue;
                                    }
                                    Ok(()) => {
                                        // H2b: restart pipeline here
                                        let factory = match &self.restart_factory {
                                            Some(f) => f,
                                            None => {
                                                send_tcp_control(
                                                    &mut stream,
                                                    ControlMessage::StreamConfigReject(
                                                        StreamConfigurationReject {
                                                            request_id: request.request_id,
                                                            reason:
                                                                StreamConfigurationErrorCode::RejectedByPeer,
                                                            message: "stream options are not supported on this host platform yet".to_owned(),
                                                        },
                                                    ),
                                                )?;
                                                continue;
                                            }
                                        };
                                        let selected = &displays[request.display_index as usize];
                                        let native_w = selected.width;
                                        let native_h = selected.height;
                                        let w = (((native_w as u64)
                                            * (request.scale_permille as u64)
                                            / 1000)
                                            & !1)
                                            .max(2)
                                            as u32;
                                        let h = (((native_h as u64)
                                            * (request.scale_permille as u64)
                                            / 1000)
                                            & !1)
                                            .max(2)
                                            as u32;
                                        let resolved_codec = request.codec;
                                        let bitrate = if request.bitrate != 0 {
                                            request.bitrate
                                        } else {
                                            active_bitrate
                                        };
                                        let output_name = displays
                                            .get(request.display_index as usize)
                                            .map(|d| d.name.clone());
                                        let restart = StreamRestart {
                                            width: w,
                                            height: h,
                                            scale_permille: request.scale_permille,
                                            fps: request.fps as u32,
                                            codec: resolved_codec,
                                            output_name,
                                            bitrate,
                                        };
                                        let source = factory(&restart);
                                        #[cfg(target_os = "windows")]
                                        if request.scale_permille != 1000
                                            && !source.supports_cpu_scaling()
                                        {
                                            send_tcp_control(&mut stream, ControlMessage::StreamConfigReject(StreamConfigurationReject {
                                                request_id: request.request_id,
                                                reason: StreamConfigurationErrorCode::UnsupportedDimensions,
                                                message: "Windows CPU capture does not support scaling".into(),
                                            }))?;
                                            continue;
                                        }
                                        let (tx, rx) =
                                            mpsc::sync_channel(SESSION_MEDIA_QUEUE_DEPTH);
                                        let new_handle = match source
                                            .start_with_capabilities(tx, peer_caps)
                                        {
                                            Ok(handle) => handle,
                                            Err(error) => {
                                                warn!(%error, "failed to start restarted media pipeline");
                                                send_tcp_control(
                                                    &mut stream,
                                                    ControlMessage::StreamConfigReject(
                                                        StreamConfigurationReject {
                                                            request_id: request.request_id,
                                                            reason:
                                                                StreamConfigurationErrorCode::RejectedByPeer,
                                                            message: format!("pipeline start error: {error}"),
                                                        },
                                                    ),
                                                )?;
                                                continue;
                                            }
                                        };
                                        #[cfg(target_os = "macos")]
                                        {
                                            if let Some(mut previous) = pending_mac.take() {
                                                previous.handle.stop();
                                            }
                                            pending_mac = Some(PendingMacPipeline {
                                                handle: new_handle,
                                                receiver: rx,
                                                restart,
                                                response: Some(StreamOptionsActive {
                                                    request_id: request.request_id,
                                                    width: w,
                                                    height: h,
                                                    fps: request.fps,
                                                    codec: resolved_codec,
                                                    display_index: request.display_index,
                                                    bitrate,
                                                }),
                                            });
                                            continue;
                                        }
                                        #[cfg(not(target_os = "macos"))]
                                        {
                                            let resolved_codec = new_handle.codec();
                                            // Hand the receiver over FIRST
                                            if let Some(ref stx) = swap_tx {
                                                let _ = stx.send((rx, w, h));
                                            } else {
                                                media_receiver = Some(rx);
                                                stream_dims = (w, h);
                                            }
                                            // THEN take old handle and stop it
                                            let old_handle = pipeline
                                                .lock()
                                                .expect("pipeline slot poisoned")
                                                .take();
                                            if let Some(mut old) = old_handle {
                                                old.stop();
                                            }
                                            if let Err(error) = new_handle.force_key_frame() {
                                                warn!(%error, "failed to force key frame on restarted media pipeline");
                                            }
                                            *pipeline.lock().expect("pipeline slot poisoned") =
                                                Some(new_handle);
                                            #[cfg(target_os = "linux")]
                                            if let Some(geometry) = probe_linux_input_geometry(
                                                restart.output_name.as_deref(),
                                            ) {
                                                input.dispatch(move |input| {
                                                    if let Err(error) = input.update_geometry(geometry) {
                                                        warn!(%error, "input geometry update failed");
                                                    }
                                                })?;
                                            }
                                            #[cfg(target_os = "macos")]
                                            if let Some(info) = restart
                                                .output_name
                                                .as_deref()
                                                .and_then(|id| id.parse().ok())
                                                .and_then(|id| {
                                                    ScreenCapture::display_info_for(id).ok()
                                                })
                                            {
                                                input.dispatch(move |input| {
                                                    reset_input(input);
                                                    *input = InputInjector::with_origin(
                                                        info.logical_width as f32,
                                                        info.logical_height as f32,
                                                        info.desktop_x as f32,
                                                        info.desktop_y as f32,
                                                    );
                                                })?;
                                            }
                                            active_bitrate = bitrate;
                                            transport_bitrate.store(
                                                u64::from(active_bitrate),
                                                Ordering::Release,
                                            );
                                            selected_output = restart.output_name.clone();
                                            active_scale = request.scale_permille;
                                            active_fps = request.fps as u32;
                                            active_codec = resolved_codec;
                                            #[cfg(target_os = "windows")]
                                            if let Some(target) = source.output_target() {
                                                input.dispatch(move |input| {
                                                    input.update_target(target);
                                                    if let Err(error) = input.refresh_virtual_desktop() {
                                                        warn!(%error, "input desktop refresh failed");
                                                    }
                                                })?;
                                            }
                                            send_tcp_control(
                                                &mut stream,
                                                ControlMessage::StreamOptionsActive(
                                                    StreamOptionsActive {
                                                        request_id: request.request_id,
                                                        width: w,
                                                        height: h,
                                                        fps: request.fps,
                                                        codec: resolved_codec,
                                                        display_index: request.display_index,
                                                        bitrate,
                                                    },
                                                ),
                                            )?;
                                            crate::host_log(&format!(
                                            "stream options applied: {w}x{h} fps={} codec={:?} display={}",
                                            request.fps, resolved_codec, request.display_index
                                        ));
                                        }
                                    }
                                }
                            }
                            ControlMessage::Ping => {
                                send_tcp_control(&mut stream, ControlMessage::Pong)?;
                            }
                            ControlMessage::Pong => last_pong = Instant::now(),
                            ControlMessage::ClipboardSyncUpdate(update) => {
                                #[cfg(any(
                                    target_os = "windows",
                                    target_os = "linux",
                                    target_os = "macos"
                                ))]
                                image_assembly.cancel();
                                #[cfg(any(
                                    target_os = "windows",
                                    target_os = "linux",
                                    target_os = "macos"
                                ))]
                                if clipboard_sync
                                    && effective_permissions.clipboard
                                    && matches!(
                                        update.direction,
                                        ClipboardSyncDirection::ClientToHost
                                            | ClipboardSyncDirection::Bidirectional
                                    )
                                {
                                    if let Some(clipboard) = clipboard.as_mut() {
                                        let applied = {
                                            #[cfg(any(target_os = "windows", target_os = "macos"))]
                                            {
                                                clipboard.apply_remote_text(&update.text)
                                            }
                                            #[cfg(target_os = "linux")]
                                            {
                                                clipboard
                                                    .apply_remote_text(&update.text, Instant::now())
                                            }
                                        };
                                        if let Err(error) = applied {
                                            warn!(%error, "failed to apply remote clipboard text");
                                        }
                                    }
                                }
                                #[cfg(not(any(
                                    target_os = "windows",
                                    target_os = "linux",
                                    target_os = "macos"
                                )))]
                                let _ = update;
                            }
                            ControlMessage::Disconnect | ControlMessage::StopStream => break,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            Ok(())
        })();

        // Stop the media pipeline before joining its consumer. `media.stop()`
        // releases the producer's event sender, which is how the UDP sender
        // thread observes a closed channel and exits. Joining first could wait
        // forever on a sender that never wakes, and because the pipeline stop
        // used to run last, that wait leaked the capture and encode workers at
        // full CPU for the rest of the daemon's life.
        #[cfg(target_os = "macos")]
        if let Some(mut pending) = pending_mac.take() {
            pending.handle.stop();
        }
        drop(media_receiver);
        audio_allowed.store(false, Ordering::SeqCst);
        if let Some(tx) = stop_sender_tx {
            let _ = tx.send(());
        }
        // Wake a sender awaiting hotplug recovery as well as one consuming media.
        drop(swap_tx);
        if let Some(mut media) = pipeline.lock().expect("pipeline slot poisoned").take() {
            media.stop();
        }
        if let Some(handle) = sender_thread.take() {
            if !join_with_deadline(handle, SENDER_JOIN_TIMEOUT) {
                warn!("UDP sender thread did not exit before the teardown deadline");
            }
        }
        // Closing dispatch drains accepted events in order, then releases held input.
        input.stop();
        state = SessionState::Closed;
        drop(sleep_guard);
        drop(guest_guard);
        if let Some(trace) = host_trace::enabled() {
            if let Err(error) = trace.dump() {
                warn!(%error, "Host diagnostic dump failed");
            }
        }
        if let Some(peer) = udp_peer {
            self.udp_recovery
                .lock()
                .expect("UDP recovery routes poisoned")
                .remove(peer);
        }
        debug!(?state, "session closed");
        result
    }

    fn discover_udp_peer(
        &self,
        tcp_peer: SocketAddr,
        udp_peer: &mut Option<SocketAddr>,
        receive_cipher: Option<&mut DatagramCipher>,
    ) -> Result<(), SessionError> {
        let Some(cipher) = receive_cipher else {
            return Ok(());
        };
        let mut routes = self
            .udp_recovery
            .lock()
            .expect("UDP recovery routes poisoned");
        for _ in 0..MAX_UDP_DISCOVERY_BURST {
            let Some((packet, peer)) = routes.discovery(&self.udp_socket, tcp_peer.ip())? else {
                return Ok(());
            };
            if udp_peer.is_some() {
                continue;
            }
            if packet.len() < ENCRYPTED_DATAGRAM_OVERHEAD {
                continue;
            }
            if let Ok((header, payload)) = cipher.open_datagram(&packet) {
                if header.packet_type == PacketType::Ping && payload.is_empty() {
                    *udp_peer = Some(peer);
                    routes.register(peer);
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

/// Maximum number of UDP registration datagrams processed per discovery step
/// before yielding back to the outer session loop to prevent TCP control starvation.
pub const MAX_UDP_DISCOVERY_BURST: usize = 32;

/// Explicit conservative UDP payload budget ensuring datagrams never exceed
/// the 1280-byte MTU standard for IPv6 and Tailscale / WireGuard virtual interfaces.
///
/// Budget calculation:
/// ```text
///   1280 bytes MTU
/// -   40 bytes IPv6 header (or 20 bytes IPv4)
/// -    8 bytes UDP header
/// = 1232 bytes maximum UDP payload under IPv6
/// ```
///
/// To provide safe headroom for IP options and encapsulation headers,
/// MahoRD targets a conservative 1200-byte UDP payload budget.
pub const UDP_PAYLOAD_BUDGET: usize = 1200;

/// Encrypted envelope overhead on the wire:
/// PacketHeader::SIZE (12 B) + udp_gcm::NONCE_SIZE (12 B) + udp_gcm::TAG_SIZE (16 B) = 40 bytes.
pub const ENCRYPTED_DATAGRAM_OVERHEAD: usize =
    PacketHeader::SIZE + ferryx_rd_net::udp_gcm::NONCE_SIZE + ferryx_rd_net::udp_gcm::TAG_SIZE;

/// Maximum plaintext payload per datagram within the conservative UDP payload budget:
/// 1200 - 40 = 1160 bytes.
pub const MAX_SENDER_PLAINTEXT_PAYLOAD: usize = UDP_PAYLOAD_BUDGET - ENCRYPTED_DATAGRAM_OVERHEAD;

/// MTU-safe sender video chunk size (1154 bytes).
/// Plaintext video chunk payload = FrameChunk::HEADER_SIZE (6 B) + data bytes.
/// Total datagram = 40 (envelope) + 6 (header) + 1154 (data) = 1200 bytes <= UDP_PAYLOAD_BUDGET.
/// Note: Wire protocol accepts up to MAX_VIDEO_CHUNK_BYTES (1382 B) on the receiver side.
pub const SENDER_MAX_VIDEO_CHUNK_BYTES: usize =
    MAX_SENDER_PLAINTEXT_PAYLOAD - ferryx_rd_proto::FrameChunk::HEADER_SIZE;

/// MTU-safe sender audio fragment size (1152 bytes).
/// Plaintext audio fragment payload = AudioFragmentHeader::SIZE (8 B) + data bytes.
/// Total datagram = 40 (envelope) + 8 (header) + 1152 (data) = 1200 bytes <= UDP_PAYLOAD_BUDGET.
/// Note: Wire protocol accepts up to MAX_AUDIO_FRAGMENT_BYTES (1380 B) on the receiver side.
pub const SENDER_MAX_AUDIO_FRAGMENT_BYTES: usize =
    MAX_SENDER_PLAINTEXT_PAYLOAD - AudioFragmentHeader::SIZE;

const _: () = assert!(SENDER_MAX_VIDEO_CHUNK_BYTES <= MAX_VIDEO_CHUNK_BYTES);
const _: () = assert!(SENDER_MAX_AUDIO_FRAGMENT_BYTES <= MAX_AUDIO_FRAGMENT_BYTES);
const _: () = assert!(
    ENCRYPTED_DATAGRAM_OVERHEAD
        + ferryx_rd_proto::FrameChunk::HEADER_SIZE
        + SENDER_MAX_VIDEO_CHUNK_BYTES
        <= UDP_PAYLOAD_BUDGET
);
const _: () = assert!(
    ENCRYPTED_DATAGRAM_OVERHEAD + AudioFragmentHeader::SIZE + SENDER_MAX_AUDIO_FRAGMENT_BYTES
        <= UDP_PAYLOAD_BUDGET
);

const VIDEO_SEND_DEADLINE: Duration = Duration::from_millis(100);
const IDR_RETRY_INTERVAL: Duration = crate::idr::IDR_INTERVAL;

fn request_forced_idr(
    limiter: &crate::idr::ForcedIdrLimiter,
    pipeline: &PipelineSlot,
    now: Instant,
) -> Result<bool, SessionError> {
    limiter.request(now, || {
        let slot = pipeline.lock().expect("pipeline slot poisoned");
        if let Some(media) = slot.as_ref() {
            media.force_key_frame()?;
            Ok(true)
        } else {
            Ok(false)
        }
    })
}

fn video_expired(frame: &VideoFrame, now: Instant) -> bool {
    // Encoding is outside the sender's control; recovery IDRs must finish.
    !frame.is_key_frame && now >= frame.encode_completed_at + VIDEO_SEND_DEADLINE
}

struct PendingVideo {
    frame: VideoFrame,
    index: usize,
    chunk_size: usize,
    parity: VecDeque<ferryx_rd_proto::FrameParity>,
    frame_id: u32,
    session_origin: Instant,
    next_at: Instant,
}

#[derive(Default)]
struct UdpSender {
    input_echo: Option<Arc<Mutex<AppliedInput>>>,
    retransmit_at: Option<Instant>,
    recovery: transport_recovery::Recovery,
    idr_limiter: Arc<crate::idr::ForcedIdrLimiter>,
    pending: Option<PendingVideo>,
    incremental: bool,
    needs_keyframe: bool,
    last_idr_request: Option<Instant>,
    send_capacity: usize,
    largest_keyframe: usize,
    bitrate: u32,
    #[cfg(target_os = "linux")]
    batch: Vec<Vec<u8>>,
    trace: Option<Arc<host_trace::Trace>>,
    trace_session: u64,
    trace_keyframe: bool,
    sequence: u32,
    frame_id: u32,
    audio_frame_id: u32,
    audio_opus_enabled: bool,
    audio_source_time_enabled: bool,
    audio_session_origin: Option<Instant>,
    audio_capture_at: Option<Instant>,
    audio_next_capture_at: Option<Instant>,
    audio_opus: Option<crate::audio_opus::AudioOpusEncoder>,
    payload: Vec<u8>,
    datagram: Vec<u8>,
    #[cfg(target_os = "macos")]
    mac_audio: Option<Arc<MacAudioSlot>>,
    #[cfg(target_os = "macos")]
    mac_wakeup: Option<Arc<crate::capture_macos::Wakeup>>,
}

impl UdpSender {
    fn send_retransmit_burst(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        now: Instant,
    ) -> Result<(), SessionError> {
        if self.retransmit_at.is_some_and(|at| now < at) {
            return Ok(());
        }
        let mut resent = 0;
        for _ in 0..self.burst_packets() {
            // Recheck at each actual send, not just at NACK admission.
            let Some(payload) = self.recovery.next(Instant::now().max(now)) else {
                break;
            };
            self.send_packet(socket, peer, cipher, PacketType::FrameChunk, &payload)?;
            resent += 1;
        }
        if resent > 0 {
            self.retransmit_at =
                Some(Instant::now().max(now) + self.burst_interval(resent * UDP_PAYLOAD_BUDGET));
        }
        Ok(())
    }

    fn send_packet(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        packet_type: PacketType,
        payload: &[u8],
    ) -> Result<(), SessionError> {
        // The shared socket is marked AF41 once at bind. Never mutate its
        // traffic class per packet: cloned sockets share it across sessions.
        self.send_selected_packet(cipher, packet_type, payload, |bytes| {
            socket.send_to(bytes, peer)
        })
    }

    fn send_selected_packet(
        &mut self,
        cipher: &mut DatagramCipher,
        packet_type: PacketType,
        payload: &[u8],
        send: impl FnOnce(&[u8]) -> io::Result<usize>,
    ) -> Result<(), SessionError> {
        self.sequence = self.sequence.wrapping_add(1);
        let header = PacketHeader::new(packet_type, self.sequence, unix_ms_u32(), 0);
        let header = header.to_bytes();
        self.datagram.clear();
        self.datagram.extend_from_slice(&header);
        cipher.seal_into(payload, &header, &mut self.datagram)?;
        let observation = self.trace.as_ref().map(|trace| {
            let frame = match packet_type {
                PacketType::FrameHeader | PacketType::FrameChunk | PacketType::AudioFrame => {
                    payload
                        .get(..4)
                        .map(|bytes| {
                            u64::from(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
                        })
                        .unwrap_or(0)
                }
                PacketType::Ping => {
                    TimestampStats::decode(payload).map_or(0, |stats| u64::from(stats.frame_id))
                }
                _ => 0,
            };
            let record = host_trace::Record {
                session: self.trace_session,
                sequence: self.sequence,
                kind: packet_type as u8,
                size: self.datagram.len(),
                frame,
                event: 1,
                keyframe: self.trace_keyframe
                    && matches!(
                        packet_type,
                        PacketType::FrameHeader | PacketType::FrameChunk | PacketType::Ping
                    ),
                ..Default::default()
            };
            trace.record(record);
            record
        });
        let result = send(&self.datagram);
        if let (Some(trace), Some(mut record)) = (&self.trace, observation) {
            match &result {
                Ok(size) => {
                    record.event = 2;
                    record.value = u64::try_from(*size).unwrap_or(u64::MAX);
                }
                Err(error) => {
                    record.event = 3;
                    record.value = error
                        .raw_os_error()
                        .map_or(0, |code| u64::from(code.unsigned_abs()));
                }
            }
            trace.record(record);
        }
        result?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn send_frame(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        width: u32,
        height: u32,
        frame: VideoFrame,
        session_origin: Instant,
    ) -> Result<(), SessionError> {
        if video_expired(&frame, Instant::now()) {
            return Err(SessionError::Io(io::Error::from(io::ErrorKind::TimedOut)));
        }
        if frame.is_key_frame && frame.data.len() > self.largest_keyframe {
            self.largest_keyframe = frame.data.len();
            if self.send_capacity > 0 && self.send_capacity < self.largest_keyframe {
                warn!(effective = self.send_capacity, largest_keyframe = self.largest_keyframe, "UDP send buffer below keyframe size; Linux: raise net.core.wmem_max; macOS: raise kern.ipc.maxsockbuf; bounded pacing remains enabled");
            }
        }
        let chunk_size = if self.recovery.enabled {
            MAX_SENDER_PLAINTEXT_PAYLOAD - ferryx_rd_proto::FrameParity::HEADER_SIZE
        } else {
            SENDER_MAX_VIDEO_CHUNK_BYTES
        };
        let chunk_count = frame.data.len().div_ceil(chunk_size);
        let frame_id = self.frame_id.wrapping_add(1);
        let header = FrameHeader {
            frame_id,
            width: width.min(u16::MAX as u32) as u16,
            height: height.min(u16::MAX as u32) as u16,
            is_key_frame: frame.is_key_frame,
            total_chunks: u16::try_from(chunk_count)
                .map_err(|_| SessionError::Store("encoded frame has too many chunks".into()))?,
            total_size: u32::try_from(frame.data.len())
                .map_err(|_| SessionError::Store("encoded frame is too large".into()))?,
        };
        let encoded_header = header.encode()?;
        self.frame_id = frame_id;
        self.trace_keyframe = frame.is_key_frame;
        if let Some(trace) = &self.trace {
            trace.record(host_trace::Record {
                session: self.trace_session,
                event: 20,
                frame: u64::from(frame_id),
                value: trace.time(frame.capture_at),
                keyframe: frame.is_key_frame,
                ..Default::default()
            });
        }
        self.send_packet(
            socket,
            peer,
            cipher,
            PacketType::FrameHeader,
            &encoded_header,
        )?;
        let parity = if self.recovery.enabled {
            let chunks: Vec<_> = frame.data.chunks(chunk_size).collect();
            ferryx_rd_proto::build_parity(
                frame_id,
                &chunks,
                ferryx_rd_proto::parity_count_for(chunks.len(), self.recovery.redundancy()),
            )
            .into()
        } else {
            VecDeque::new()
        };
        self.recovery.begin(frame_id, frame.encode_completed_at);
        self.pending = Some(PendingVideo {
            frame,
            index: 0,
            chunk_size,
            parity,
            frame_id,
            session_origin,
            next_at: Instant::now(),
        });
        self.send_video_burst(socket, peer, cipher)?;
        // Small standalone sends retain the synchronous packetizer contract;
        // live sessions return after each bounded burst for audio dispatch.
        while !self.incremental && self.pending.is_some() {
            self.send_video_burst(socket, peer, cipher)?;
        }
        Ok(())
    }

    fn send_video_burst(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
    ) -> Result<(), SessionError> {
        let Some(mut pending) = self.pending.take() else {
            return Ok(());
        };
        if video_expired(&pending.frame, Instant::now()) {
            return Err(SessionError::Io(io::Error::from(io::ErrorKind::TimedOut)));
        }
        let chunks = pending.frame.data.len().div_ceil(pending.chunk_size);
        let end = (pending.index + self.burst_packets()).min(chunks);
        #[cfg(target_os = "linux")]
        let mut batch = std::mem::take(&mut self.batch);
        #[cfg(target_os = "linux")]
        let batching = end - pending.index > 2 && self.trace.is_none();
        #[cfg(target_os = "linux")]
        if batching {
            batch.resize_with(batch.len().max(end - pending.index), || {
                Vec::with_capacity(UDP_PAYLOAD_BUDGET)
            });
        }
        #[cfg(target_os = "macos")]
        self.send_mac_audio(socket, peer, cipher)?;
        for index in pending.index..end {
            let bytes = &pending.frame.data[index * pending.chunk_size
                ..((index + 1) * pending.chunk_size).min(pending.frame.data.len())];
            let mut payload = std::mem::take(&mut self.payload);
            payload.clear();
            payload.extend_from_slice(&pending.frame_id.to_le_bytes());
            payload.extend_from_slice(&(index as u16).to_le_bytes());
            payload.extend_from_slice(bytes);
            #[cfg(target_os = "linux")]
            let sent = if batching {
                self.send_selected_packet(cipher, PacketType::FrameChunk, &payload, |bytes| {
                    let datagram = &mut batch[index - pending.index];
                    datagram.clear();
                    datagram.extend_from_slice(bytes);
                    Ok(bytes.len())
                })
            } else {
                self.send_packet(socket, peer, cipher, PacketType::FrameChunk, &payload)
            };
            #[cfg(not(target_os = "linux"))]
            let sent = self.send_packet(socket, peer, cipher, PacketType::FrameChunk, &payload);
            self.recovery.retain(index as u16, &payload);
            self.payload = payload;
            #[cfg(target_os = "linux")]
            if let Err(error) = sent {
                self.batch = batch;
                return Err(error);
            }
            #[cfg(not(target_os = "linux"))]
            sent?;
        }
        #[cfg(target_os = "linux")]
        {
            let count = if batching { end - pending.index } else { 0 };
            let result = ferryx_rd_net::udp_gcm::send_datagram_batch(socket, peer, &batch[..count]);
            self.batch = batch;
            if result? != count {
                return Err(SessionError::Io(io::Error::from(io::ErrorKind::WouldBlock)));
            }
        }
        let sent_bytes = (end - pending.index) * UDP_PAYLOAD_BUDGET;
        pending.index = end;
        if end < chunks {
            pending.next_at = Instant::now() + self.burst_interval(sent_bytes);
            self.pending = Some(pending);
            return Ok(());
        }
        if !pending.parity.is_empty() {
            for _ in 0..self.burst_packets() {
                let Some(parity) = pending.parity.pop_front() else {
                    break;
                };
                self.send_packet(
                    socket,
                    peer,
                    cipher,
                    PacketType::FrameParity,
                    &parity.encode()?,
                )?;
            }
            pending.next_at =
                Instant::now() + self.burst_interval(self.burst_packets() * UDP_PAYLOAD_BUDGET);
            self.pending = Some(pending);
            return Ok(());
        }
        let stats = TimestampStats {
            frame_id: pending.frame_id,
            capture_us: monotonic_us(pending.session_origin, pending.frame.capture_at),
            encode_start_us: monotonic_us(pending.session_origin, pending.frame.encode_started_at),
            encode_end_us: monotonic_us(pending.session_origin, pending.frame.encode_completed_at),
            send_us: monotonic_us(pending.session_origin, Instant::now()),
        };
        if let Some(applied) = &self.input_echo {
            let echo = applied.lock().expect("input echo poisoned").for_frame(
                pending.frame_id,
                pending.frame.capture_at,
                pending.session_origin,
            );
            if let Some(echo) = echo {
                self.send_packet(socket, peer, cipher, PacketType::Ping, &echo.encode())?;
            }
        }
        self.send_packet(socket, peer, cipher, PacketType::Ping, &stats.encode())?;
        if pending.frame.is_key_frame {
            self.needs_keyframe = false;
            self.last_idr_request = None;
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn send_mac_audio(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
    ) -> Result<(), SessionError> {
        let packet = self.mac_audio.as_ref().and_then(|slot| {
            let packet = slot.take_timed(Instant::now());
            let skipped = slot.skipped.swap(0, Ordering::Relaxed);
            if !self.audio_opus_enabled {
                self.audio_frame_id = self.audio_frame_id.wrapping_add(skipped);
            }
            packet
        });
        if let Some((captured_at, bytes)) = packet {
            self.audio_capture_at = Some(captured_at);
            self.send_audio(socket, peer, cipher, &bytes)?;
        }
        Ok(())
    }

    fn burst_packets(&self) -> usize {
        (self.send_capacity / (UDP_PAYLOAD_BUDGET * 4)).clamp(2, 8)
    }

    fn burst_interval(&self, bytes: usize) -> Duration {
        // Allow a bounded 4x keyframe catch-up rate, capped by the observed
        // socket capacity per 2ms. Small buffers therefore slow bursts down.
        let rate = f64::from(self.bitrate.max(1_000_000)) * 4.0;
        let capacity_rate = self.send_capacity.max(UDP_PAYLOAD_BUDGET) as f64 * 8.0 / 0.002;
        Duration::from_secs_f64(bytes as f64 * 8.0 / rate.min(capacity_rate))
    }

    fn invalidate_reference(&mut self, pipeline: &PipelineSlot) {
        self.pending = None;
        self.needs_keyframe = true;
        self.request_recovery(pipeline, Instant::now());
    }

    fn request_recovery(&mut self, pipeline: &PipelineSlot, now: Instant) {
        if self.needs_keyframe
            && self
                .last_idr_request
                .map_or(true, |last| now.duration_since(last) >= IDR_RETRY_INTERVAL)
            && !self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.frame.is_key_frame)
        {
            self.last_idr_request = Some(now);
            if let Err(error) = request_forced_idr(&self.idr_limiter, pipeline, now) {
                warn!(%error, "transport IDR request failed");
            }
        }
    }

    fn send_cursor(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        cursor: &CursorUpdate,
        shape_id: Option<u32>,
    ) -> Result<(), SessionError> {
        let payload = cursor.encode_with_shape(shape_id)?;
        self.send_packet(socket, peer, cipher, PacketType::CursorUpdate, &payload)
    }

    fn send_audio(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        bytes: &[u8],
    ) -> Result<(), SessionError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let captured_at = self.audio_capture_at.take().unwrap_or_else(Instant::now);
        if self.audio_opus_enabled {
            let buffered = self
                .audio_opus
                .as_ref()
                .map_or(0, |encoder| encoder.buffered_frames());
            let expected = self
                .audio_next_capture_at
                .map(|at| at + Duration::from_micros(buffered as u64 * 1_000_000 / 48_000));
            if let Some(next) = expected.filter(|next| {
                captured_at.saturating_duration_since(*next) >= Duration::from_millis(10)
            }) {
                let missing = captured_at.saturating_duration_since(next).as_micros() / 10_000;
                self.audio_frame_id = self
                    .audio_frame_id
                    .wrapping_add(missing.min(u128::from(u32::MAX)) as u32);
                self.audio_opus = None;
                self.audio_next_capture_at = Some(captured_at);
            } else if buffered == 0 {
                self.audio_next_capture_at = Some(captured_at);
            }
        }
        if self.audio_next_capture_at.is_none() {
            self.audio_next_capture_at = Some(captured_at);
        }
        if self.audio_opus_enabled {
            if self.audio_opus.is_none() {
                self.audio_opus = Some(
                    crate::audio_opus::AudioOpusEncoder::new()
                        .map_err(|error| SessionError::Store(format!("Opus encoder: {error}")))?,
                );
            }
            let packets = self
                .audio_opus
                .as_mut()
                .expect("initialized encoder")
                .encode_pcm(bytes)
                .map_err(|error| SessionError::Store(format!("Opus encode: {error}")))?;
            for packet in packets {
                let mut payload = Vec::with_capacity(packet.len() + 1);
                payload.push(1); // AUDIO_OPUS: restricted-lowdelay Opus, 48k stereo, 10ms.
                payload.extend_from_slice(&packet);
                self.send_timed_audio_payload(socket, peer, cipher, &payload)?;
                self.audio_next_capture_at = self
                    .audio_next_capture_at
                    .map(|at| at + Duration::from_millis(10));
            }
            return Ok(());
        }
        self.audio_next_capture_at = Some(captured_at);
        self.send_timed_audio_payload(socket, peer, cipher, bytes)
    }

    fn send_timed_audio_payload(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        bytes: &[u8],
    ) -> Result<(), SessionError> {
        if self.audio_source_time_enabled {
            let origin = self
                .audio_session_origin
                .expect("negotiated audio session origin");
            let capture = self.audio_next_capture_at.expect("audio capture timestamp");
            let payload = ferryx_rd_proto::audio_source_prefix(
                bytes,
                monotonic_us(origin, capture),
                Capabilities::AUDIO_SOURCE_TIME,
            );
            self.send_audio_payload(socket, peer, cipher, &payload)
        } else {
            self.send_audio_payload(socket, peer, cipher, bytes)
        }
    }

    fn send_audio_payload(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        cipher: &mut DatagramCipher,
        bytes: &[u8],
    ) -> Result<(), SessionError> {
        self.audio_frame_id = self.audio_frame_id.wrapping_add(1);
        let fragment_count = bytes.len().div_ceil(SENDER_MAX_AUDIO_FRAGMENT_BYTES);
        let fragment_count = u16::try_from(fragment_count)
            .map_err(|_| SessionError::Store("audio frame has too many fragments".into()))?;
        for (index, data) in bytes.chunks(SENDER_MAX_AUDIO_FRAGMENT_BYTES).enumerate() {
            let header = AudioFragmentHeader {
                frame_id: self.audio_frame_id,
                fragment_index: index as u16,
                fragment_count,
            }
            .to_bytes()?;
            let mut payload = std::mem::take(&mut self.payload);
            payload.clear();
            payload.extend_from_slice(&header);
            payload.extend_from_slice(data);
            let sent = self.send_packet(socket, peer, cipher, PacketType::AudioFrame, &payload);
            self.payload = payload;
            sent?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
type PlatformInput = InputInjector;
#[cfg(target_os = "windows")]
type PlatformInput = WindowsInputInjector;
#[cfg(target_os = "linux")]
type PlatformInput = LinuxInputInjector;

type InputJob = Box<dyn FnOnce(&mut PlatformInput) + Send>;

enum QueuedInput {
    Job(InputJob),
    Event(u32, InputEvent),
}

#[derive(Default)]
struct InputQueue {
    pending: std::collections::VecDeque<QueuedInput>,
    closed: bool,
}

impl InputQueue {
    fn coalesce(&mut self, sequence: u32, event: &InputEvent) -> bool {
        let Some(QueuedInput::Event(previous_sequence, previous)) = self.pending.back_mut() else {
            return false;
        };
        if previous.event_type != event.event_type || previous.modifiers != event.modifiers {
            return false;
        }
        match event.event_type {
            ferryx_rd_proto::InputEventType::MouseMove => {
                previous.x = event.x;
                previous.y = event.y;
            }
            ferryx_rd_proto::InputEventType::RelativeMove => {
                previous.scroll_dx += event.scroll_dx;
                previous.scroll_dy += event.scroll_dy;
            }
            _ => return false,
        }
        *previous_sequence = sequence;
        true
    }
}

#[derive(Default)]
struct AppliedInput {
    history: VecDeque<(u64, Instant)>,
}

impl AppliedInput {
    fn record(&mut self, sequence: u64, at: Instant) {
        if self
            .history
            .back()
            .map_or(true, |(previous, _)| sequence > *previous)
        {
            if self.history.len() == 256 {
                self.history.pop_front();
            }
            self.history.push_back((sequence, at));
        }
    }

    fn for_frame(
        &self,
        frame_id: u32,
        captured: Instant,
        origin: Instant,
    ) -> Option<ferryx_rd_proto::InputEcho> {
        let &(sequence, injected) = self.history.iter().rev().find(|(_, at)| captured > *at)?;
        Some(ferryx_rd_proto::InputEcho {
            frame_id,
            last_applied_input_seq: sequence,
            injection_timestamp_us: monotonic_us(origin, injected),
        })
    }
}

#[cfg(test)]
mod applied_input_tests {
    use super::*;

    #[test]
    fn echo_is_monotonic_and_requires_capture_after_injection() {
        let origin = Instant::now();
        let injected = origin + Duration::from_micros(100);
        let mut applied = AppliedInput::default();
        assert!(applied.for_frame(1, injected, origin).is_none());
        applied.record(10, injected);
        applied.record(9, injected + Duration::from_micros(10));
        assert!(applied.for_frame(1, origin, origin).is_none());
        assert!(applied.for_frame(1, injected, origin).is_none());
        let echo = applied
            .for_frame(2, injected + Duration::from_micros(1), origin)
            .unwrap();
        assert_eq!(echo.last_applied_input_seq, 10);
        assert_eq!(echo.injection_timestamp_us, 100);
        applied.record(11, injected + Duration::from_micros(20));
        // A newer injection must not hide an older eligible capture.
        assert_eq!(
            applied
                .for_frame(3, injected + Duration::from_micros(15), origin)
                .unwrap()
                .last_applied_input_seq,
            10
        );
        assert_eq!(
            applied
                .for_frame(4, injected + Duration::from_micros(21), origin)
                .unwrap()
                .last_applied_input_seq,
            11
        );
    }
}

struct InputDispatch {
    ack_tx: mpsc::Sender<InputAckMessage>,
    applied: Arc<Mutex<AppliedInput>>,
    queue: Arc<(Mutex<InputQueue>, std::sync::Condvar)>,
    acks: Receiver<InputAckMessage>,
    completed: thread::JoinHandle<()>,
}

fn input_ack(sequence: u32, event: &InputEvent, success: bool) -> Option<InputAckMessage> {
    let motion = matches!(
        event.event_type,
        ferryx_rd_proto::InputEventType::MouseMove | ferryx_rd_proto::InputEventType::RelativeMove
    );
    (!success || !motion).then_some(InputAckMessage {
        sequence,
        success,
        error_code: if success { 0 } else { 1 },
    })
}

fn reset_input(input: &mut PlatformInput) {
    let event = InputEvent {
        event_type: ferryx_rd_proto::InputEventType::Reset,
        x: 0.0,
        y: 0.0,
        key_code: 0,
        modifiers: ferryx_rd_proto::Modifiers::empty(),
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    };
    inject_input(&event, |event| input.inject(event));
}

impl InputDispatch {
    fn start(
        create: impl FnOnce() -> Result<PlatformInput, SessionError> + Send + 'static,
    ) -> Result<Self, SessionError> {
        let applied = Arc::new(Mutex::new(AppliedInput::default()));
        let queue = Arc::new((Mutex::new(InputQueue::default()), std::sync::Condvar::new()));
        let worker_queue = queue.clone();
        // Acknowledgements are small and must not block input behind TLS writes.
        // Discrete jobs remain bounded by the queue; motion success produces no ACK.
        let (ack_tx, acks) = mpsc::channel();
        let worker_ack_tx = ack_tx.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let completed = thread::Builder::new()
            .name("maho-input".into())
            .spawn(move || {
                let mut input = match create() {
                    Ok(input) => {
                        let _ = ready_tx.send(Ok(()));
                        input
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                let (lock, changed) = &*worker_queue;
                loop {
                    let mut queue = lock.lock().expect("input queue poisoned");
                    while queue.pending.is_empty() && !queue.closed {
                        let (next, timeout) = changed
                            .wait_timeout(queue, Duration::from_millis(5))
                            .expect("input queue poisoned");
                        queue = next;
                        if timeout.timed_out() {
                            drop(queue);
                            #[cfg(target_os = "linux")]
                            input.flush_idle_text(Instant::now());
                            queue = lock.lock().expect("input queue poisoned");
                        }
                    }
                    let Some(job) = queue.pending.pop_front() else {
                        break;
                    };
                    changed.notify_all();
                    drop(queue);
                    match job {
                        QueuedInput::Job(job) => job(&mut input),
                        QueuedInput::Event(sequence, event) => {
                            let success = inject_input(&event, |event| input.inject(event));
                            if let Some(ack) = input_ack(sequence, &event, success) {
                                if worker_ack_tx.send(ack).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                reset_input(&mut input);
            })?;
        ready_rx.recv().map_err(|_| SessionError::MediaStopped)??;
        Ok(Self {
            ack_tx,
            applied,
            queue,
            acks,
            completed,
        })
    }

    fn enqueue(&self, job: QueuedInput) -> Result<(), SessionError> {
        let (lock, changed) = &*self.queue;
        let mut queue = lock.lock().expect("input queue poisoned");
        if let QueuedInput::Event(sequence, event) = &job {
            if queue.coalesce(*sequence, event) {
                return Ok(());
            }
        }
        let (mut queue, timeout) = changed
            .wait_timeout_while(queue, Duration::from_millis(250), |q| {
                q.pending.len() >= 256 && !q.closed
            })
            .expect("input queue poisoned");
        if queue.closed || (timeout.timed_out() && queue.pending.len() >= 256) {
            warn!("input injector unavailable after bounded queue wait; closing session");
            return Err(SessionError::MediaStopped);
        }
        queue.pending.push_back(job);
        changed.notify_all();
        Ok(())
    }

    fn dispatch(
        &self,
        job: impl FnOnce(&mut PlatformInput) + Send + 'static,
    ) -> Result<(), SessionError> {
        self.enqueue(QueuedInput::Job(Box::new(job)))
    }

    fn inject_batch(
        &self,
        sequence: u32,
        input_sequence: u64,
        events: Vec<InputEvent>,
    ) -> Result<(), SessionError> {
        if events.is_empty() {
            return Ok(());
        }
        let applied = self.applied.clone();
        let ack_tx = self.ack_tx.clone();
        self.dispatch(move |input| {
            let mut success = true;
            for event in events {
                let injected = inject_input(&event, |event| input.inject(event));
                success &= injected;
                if let Some(ack) = input_ack(sequence, &event, injected) {
                    if ack_tx.send(ack).is_err() {
                        return;
                    }
                }
            }
            if success {
                applied
                    .lock()
                    .expect("input echo poisoned")
                    .record(input_sequence, Instant::now());
            } else {
                warn!(sequence, "input batch failed; no input echo recorded");
            }
        })
    }

    fn inject(&self, sequence: u32, event: InputEvent) -> Result<(), SessionError> {
        self.enqueue(QueuedInput::Event(sequence, event))
    }

    fn stop(self) {
        let (lock, changed) = &*self.queue;
        lock.lock().expect("input queue poisoned").closed = true;
        changed.notify_all();
        if !join_with_deadline(self.completed, SENDER_JOIN_TIMEOUT) {
            warn!("input worker did not exit before teardown deadline");
        }
    }
}

fn inject_input<E: std::fmt::Display>(
    event: &InputEvent,
    inject: impl FnOnce(&InputEvent) -> Result<(), E>,
) -> bool {
    tracing::trace!(?event, "Host received input event");
    match inject(event) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "input event was not injected");
            false
        }
    }
}

fn split_packet(packet: &[u8]) -> Result<(PacketHeader, &[u8]), SessionError> {
    if packet.len() < PacketHeader::SIZE {
        return Err(SessionError::Codec(ferryx_rd_proto::CodecError::Truncated {
            field: "packet header",
            needed: PacketHeader::SIZE,
            remaining: packet.len(),
        }));
    }
    let header = PacketHeader::decode(&packet[..PacketHeader::SIZE])?;
    Ok((header, &packet[PacketHeader::SIZE..]))
}

fn send_tcp_packet(
    stream: &mut ferryx_rd_net::TlsPskStream<TcpStream>,
    packet_type: PacketType,
    payload: &[u8],
) -> Result<(), SessionError> {
    let mut packet = PacketHeader::new(packet_type, 0, unix_ms_u32(), 0).encode()?;
    packet.extend_from_slice(payload);
    stream.write_frame(&packet)?;
    Ok(())
}

fn send_tcp_control(
    stream: &mut ferryx_rd_net::TlsPskStream<TcpStream>,
    message: ControlMessage,
) -> Result<(), SessionError> {
    send_tcp_packet(stream, PacketType::Control, &message.encode()?)
}

fn send_pairing_reject(
    stream: &mut ferryx_rd_net::TlsPskStream<TcpStream>,
    reason: PairingRejectReason,
) -> Result<(), SessionError> {
    send_tcp_packet(
        stream,
        PacketType::PairingReject,
        &PairingReject { reason }.encode()?,
    )
}

fn random_key() -> [u8; 32] {
    let mut key = [0_u8; 32];
    rand::rng().fill_bytes(&mut key);
    key
}

fn unix_ms_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn unix_ms_u32() -> u32 {
    unix_ms_u64() as u32
}

fn monotonic_us(origin: Instant, value: Instant) -> u64 {
    value
        .checked_duration_since(origin)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferryx_rd_proto::{AudioFragment, FrameChunk, StreamCodecPreference, StreamOptionsRequest};
    mod sender_trace {
        include!("sender_trace_tests.rs");
    }
    mod sender_packetization {
        include!("sender_packetization_tests.rs");
    }

    fn peek_datagram(socket: &UdpSocket, buffer: &mut [u8]) -> (usize, SocketAddr) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match socket.peek_from(buffer) {
                Ok(result) => return result,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "datagram did not arrive within 2s"
                    );
                    thread::yield_now();
                }
                Err(error) => panic!("peek_from failed: {error}"),
            }
        }
    }

    #[test]
    fn capture_failure_labels_preserve_secure_desktop_recovery() {
        use crate::windows_session::{capture_recovery, CaptureFailure, CaptureRecovery};

        for (label, expected) in [
            ("access_lost", CaptureFailure::AccessLost),
            ("access_denied", CaptureFailure::AccessDenied),
            ("refresh_failure", CaptureFailure::RefreshFailure),
            ("capture", CaptureFailure::Other),
            ("", CaptureFailure::Other),
            ("ACCESS_DENIED", CaptureFailure::Other),
        ] {
            assert_eq!(capture_failure_from_label(label), expected);
        }
        for consecutive in [0, 10_000, u32::MAX] {
            assert!(matches!(
                capture_recovery(capture_failure_from_label("access_denied"), consecutive),
                CaptureRecovery::Reacquire(_)
            ));
        }
    }

    #[test]
    fn service_pairing_store_uses_machine_wide_path() {
        let program_data = "C:/ProgramData";
        let path = PairingStore::service_default_path(program_data);
        assert_eq!(
            path,
            crate::windows_session::service_store_path(program_data)
        );
        assert_eq!(
            path,
            PathBuf::from(program_data)
                .join("MahoRD")
                .join("host-authorizations.json")
        );
    }

    #[test]
    fn bind_synthetic_does_not_start_mdns_advertisement() {
        let directory = tempdir().unwrap();
        let (consent, _) = mpsc::channel();
        let mut config = test_config(
            PairingStore::new(directory.path().join("keys.json")),
            consent,
        );
        config.tcp_addr = "127.0.0.1:0".parse().unwrap();
        config.udp_addr = "127.0.0.1:0".parse().unwrap();
        let server = HostServer::bind_synthetic(config, 1).unwrap();
        assert!(!server.is_advertising());
    }

    #[test]
    fn bind_with_advertising_fallback_keeps_host_usable() {
        let directory = tempdir().unwrap();
        let (consent, _) = mpsc::channel();
        let mut config = test_config(
            PairingStore::new(directory.path().join("keys.json")),
            consent,
        );
        config.tcp_addr = "127.0.0.1:0".parse().unwrap();
        config.udp_addr = "127.0.0.1:0".parse().unwrap();
        let source = Arc::new(SyntheticMediaSource {
            frame_count: 1,
            restart_starts: Arc::default(),
        });
        let server = HostServer::bind_with_media_advertising(config, source, true).unwrap();
        assert!(server.tcp_addr().is_ok());
        assert!(server.udp_addr().is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn input_dispatch_is_ordered_bounded_and_independent_of_ack_drain() {
        let worker = InputDispatch::start(|| Ok(InputInjector::new(100.0, 100.0))).unwrap();
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        worker
            .dispatch(move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            })
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        for index in 0..256 {
            let order = order.clone();
            worker
                .dispatch(move |_| order.lock().unwrap().push(index))
                .unwrap();
        }
        let release = thread::spawn(move || {
            // The exact queue-full condition is established above; releasing the
            // worker makes capacity available without a timing-based sleep.
            release_tx.send(()).unwrap();
        });
        worker.dispatch(|_| {}).unwrap();
        release.join().unwrap();
        worker.stop();
        assert_eq!(*order.lock().unwrap(), (0..256).collect::<Vec<_>>());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn udp_motion_authentication_routes_and_dispatch_barrier_order() {
        let directory = tempdir().unwrap();
        let (consent, _) = mpsc::channel();
        let mut config = test_config(
            PairingStore::new(directory.path().join("keys.json")),
            consent,
        );
        config.tcp_addr = "127.0.0.1:0".parse().unwrap();
        config.udp_addr = "127.0.0.1:0".parse().unwrap();
        let server = HostServer::bind_synthetic(config, 1).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        let peer = client.local_addr().unwrap();
        server.udp_recovery.lock().unwrap().register(peer);
        let mut send = DatagramCipher::derive(&[8; 32], &[9; 16], Direction::ClientToHost).unwrap();
        let mut receive =
            DatagramCipher::derive(&[8; 32], &[9; 16], Direction::ClientToHost).unwrap();
        let worker = InputDispatch::start(|| Ok(InputInjector::new(100.0, 100.0))).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        worker
            .dispatch(move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            })
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut journal = ferryx_rd_proto::InputJournal::default();
        let mut order = ferryx_rd_proto::InputOrder::default();
        let mut motion = InputEvent::decode(&[0; InputEvent::SIZE]).unwrap();
        motion.x = 0.25;
        let batch = journal.append(&[motion]);
        let packet = send
            .seal_datagram(
                &PacketHeader::new(PacketType::InputBatch, 1, 0, 0),
                &batch.encode().unwrap(),
            )
            .unwrap();
        client.send_to(&packet, server.udp_addr().unwrap()).unwrap();
        // Exact kernel readiness, no sleep or scheduling assumption.
        let mut peek = [0; 2048];
        server.udp_socket.set_nonblocking(false).unwrap();
        server
            .udp_socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        peek_datagram(&server.udp_socket, &mut peek);
        server.udp_socket.set_nonblocking(true).unwrap();
        let permissions = GuestPermissions {
            mouse: true,
            keyboard: true,
            gamepad: false,
            clipboard: false,
            audio: false,
        };
        server
            .receive_udp_datagrams(
                peer,
                &mut receive,
                None,
                Some(udp_input::UdpInputSink {
                    order: &mut order,
                    input: &worker,
                    echo: false,
                    permissions,
                }),
            )
            .unwrap();
        // Reliable barrier recovers a later lost move before the button job.
        motion.x = 0.75;
        journal.append(&[motion]);
        for event in order.accept(&journal.checkpoint(), true) {
            worker.inject(2, event).unwrap();
        }
        worker.dispatch(|_| {}).unwrap();
        let queue = worker.queue.0.lock().unwrap();
        assert_eq!(queue.pending.len(), 2);
        let QueuedInput::Event(_, latest) = queue.pending.front().unwrap() else {
            panic!("motion must precede discrete job");
        };
        assert_eq!(latest.x, 0.75);
        assert!(matches!(queue.pending.back(), Some(QueuedInput::Job(_))));
        drop(queue);
        release_tx.send(()).unwrap();
        worker.stop();
    }

    #[test]
    fn motion_coalesces_without_crossing_discrete_barriers() {
        let mut queue = InputQueue::default();
        let mut event = InputEvent::decode(&[0; InputEvent::SIZE]).unwrap();
        event.event_type = ferryx_rd_proto::InputEventType::MouseMove;
        queue.pending.push_back(QueuedInput::Event(1, event));
        for sequence in 2..10_000 {
            event.x = sequence as f32;
            assert!(queue.coalesce(sequence, &event));
        }
        assert_eq!(queue.pending.len(), 1);
        let QueuedInput::Event(sequence, latest) = queue.pending.pop_front().unwrap() else {
            panic!("motion")
        };
        assert_eq!(sequence, 9_999);
        assert_eq!(latest.x, 9_999.0);
        event.event_type = ferryx_rd_proto::InputEventType::RelativeMove;
        event.scroll_dx = 2.0;
        event.scroll_dy = -3.0;
        queue.pending.push_back(QueuedInput::Event(1, event));
        assert!(queue.coalesce(2, &event));
        let QueuedInput::Event(_, summed) = queue.pending.front().unwrap() else {
            panic!("motion")
        };
        assert_eq!((summed.scroll_dx, summed.scroll_dy), (4.0, -6.0));
        queue.pending.push_back(QueuedInput::Job(Box::new(|_| {})));
        assert!(!queue.coalesce(3, &event));
    }

    #[test]
    fn successful_motion_has_no_ack_but_errors_and_keys_do() {
        let mut event = InputEvent::decode(&[0; InputEvent::SIZE]).unwrap();
        for kind in [
            ferryx_rd_proto::InputEventType::MouseMove,
            ferryx_rd_proto::InputEventType::RelativeMove,
        ] {
            event.event_type = kind;
            assert!(input_ack(7, &event, true).is_none());
            assert_eq!(input_ack(7, &event, false).unwrap().error_code, 1);
        }
        event.event_type = ferryx_rd_proto::InputEventType::KeyUp;
        assert!(input_ack(7, &event, true).unwrap().success);
    }

    #[test]
    fn client_and_transport_idrs_share_session_limiter() {
        struct Recovery(Arc<AtomicUsize>);
        impl MediaHandle for Recovery {
            fn force_key_frame(&self) -> Result<(), SessionError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn update_bitrate(&self, _: u32) -> Result<(), SessionError> {
                Ok(())
            }
            fn stop(&mut self) {}
        }
        let requests = Arc::new(AtomicUsize::new(0));
        let pipeline: PipelineSlot =
            Arc::new(Mutex::new(Some(Box::new(Recovery(requests.clone())))));
        let limiter = Arc::new(crate::idr::ForcedIdrLimiter::default());
        let mut sender = UdpSender {
            idr_limiter: Arc::clone(&limiter),
            needs_keyframe: true,
            ..Default::default()
        };
        let now = Instant::now();
        assert!(request_forced_idr(&limiter, &pipeline, now).unwrap());
        sender.request_recovery(&pipeline, now);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        sender.request_recovery(&pipeline, now + IDR_RETRY_INTERVAL);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert!(!request_forced_idr(&limiter, &pipeline, now + IDR_RETRY_INTERVAL).unwrap());
        // Startup/restart requests intentionally bypass the reactive budget.
        pipeline
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .force_key_frame()
            .unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 3);
        assert!(!request_forced_idr(&limiter, &pipeline, now + IDR_RETRY_INTERVAL).unwrap());
    }

    #[test]
    fn transport_deadline_precedes_packetization_and_recovers_once() {
        struct Recovery(Arc<AtomicUsize>);
        impl MediaHandle for Recovery {
            fn force_key_frame(&self) -> Result<(), SessionError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn update_bitrate(&self, _: u32) -> Result<(), SessionError> {
                Ok(())
            }
            fn stop(&mut self) {}
        }
        let requests = Arc::new(AtomicUsize::new(0));
        let pipeline: PipelineSlot =
            Arc::new(Mutex::new(Some(Box::new(Recovery(requests.clone())))));
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut sender = UdpSender::default();
        let mut cipher =
            DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
        let old = Instant::now() - VIDEO_SEND_DEADLINE;
        let frame = VideoFrame {
            data: vec![1],
            is_key_frame: false,
            capture_at: old,
            encode_started_at: old,
            encode_completed_at: old,
        };
        assert!(sender
            .send_frame(&tx, tx.local_addr().unwrap(), &mut cipher, 1, 1, frame, old)
            .is_err());
        assert_eq!(sender.sequence, 0);
        sender.invalidate_reference(&pipeline);
        sender.invalidate_reference(&pipeline);
        assert!(sender.needs_keyframe);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        // A recovery IDR can still fail at the socket. Losing that IDR must
        // not latch recovery permanently: another request follows the limiter.
        sender.pending = Some(PendingVideo {
            frame: VideoFrame {
                data: vec![1],
                is_key_frame: true,
                capture_at: old,
                encode_started_at: old,
                encode_completed_at: old,
            },
            index: 0,
            chunk_size: SENDER_MAX_VIDEO_CHUNK_BYTES,
            parity: VecDeque::new(),
            frame_id: 1,
            session_origin: old,
            next_at: old,
        });
        sender.invalidate_reference(&pipeline);
        let retry_at = sender.last_idr_request.unwrap() + IDR_RETRY_INTERVAL;
        sender.request_recovery(&pipeline, retry_at);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        let fresh = VideoFrame {
            data: vec![1],
            is_key_frame: false,
            capture_at: old - Duration::from_secs(1),
            encode_started_at: old,
            encode_completed_at: Instant::now(),
        };
        sender
            .send_frame(&tx, tx.local_addr().unwrap(), &mut cipher, 1, 1, fresh, old)
            .unwrap();
        let recovery = VideoFrame {
            data: vec![1],
            is_key_frame: true,
            capture_at: old,
            encode_started_at: old,
            encode_completed_at: old,
        };
        sender
            .send_frame(
                &tx,
                tx.local_addr().unwrap(),
                &mut cipher,
                1,
                1,
                recovery,
                old,
            )
            .unwrap();
        assert!(!sender.needs_keyframe);
    }

    #[test]
    fn source_timestamp_matches_video_session_origin() {
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut cipher =
            DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
        let mut decipher =
            DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
        let origin = Instant::now();
        let mut sender = UdpSender {
            audio_source_time_enabled: true,
            audio_session_origin: Some(origin),
            audio_capture_at: Some(origin + Duration::from_micros(123456)),
            ..Default::default()
        };
        sender
            .send_audio(&tx, rx.local_addr().unwrap(), &mut cipher, &[0; 8])
            .unwrap();
        let mut datagram = [0; 2048];
        let size = rx.recv(&mut datagram).unwrap();
        let (_, payload) = decipher.open_datagram(&datagram[..size]).unwrap();
        let fragment = AudioFragment::decode(&payload).unwrap();
        let (time, pcm) =
            ferryx_rd_proto::split_audio_source_time(&fragment.data, Capabilities::AUDIO_SOURCE_TIME)
                .unwrap();
        assert_eq!(time, Some(123456));
        assert_eq!(pcm, &[0; 8]);
    }

    #[test]
    fn negotiated_opus_and_legacy_pcm_use_distinct_payloads() {
        for opus in [false, true] {
            let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
            let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
            rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut cipher =
                DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
            let mut decipher =
                DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
            let mut sender = UdpSender {
                audio_opus_enabled: opus,
                ..Default::default()
            };
            let pcm = vec![0; 480 * 2 * 4];
            sender
                .send_audio(&tx, rx.local_addr().unwrap(), &mut cipher, &pcm)
                .unwrap();
            let mut datagram = [0; 2048];
            let size = rx.recv(&mut datagram).unwrap();
            let (header, payload) = decipher.open_datagram(&datagram[..size]).unwrap();
            assert_eq!(header.packet_type, PacketType::AudioFrame);
            let fragment = AudioFragment::decode(&payload).unwrap();
            if opus {
                assert_eq!(fragment.data[0], 1);
                assert_eq!(fragment.header.fragment_count, 1);
                assert!(fragment.data.len() < 500);
            } else {
                assert_eq!(fragment.data, pcm[..SENDER_MAX_AUDIO_FRAGMENT_BYTES]);
                assert!(fragment.header.fragment_count > 1);
            }
        }
    }

    #[test]
    fn video_bursts_leave_audio_a_dispatch_opportunity() {
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let peer = rx.local_addr().unwrap();
        let mut cipher =
            DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
        let mut decipher =
            DatagramCipher::derive(&[1; 32], &[2; 16], Direction::HostToClient).unwrap();
        let mut sender = UdpSender {
            incremental: true,
            send_capacity: 64 * 1024,
            bitrate: 8_000_000,
            ..Default::default()
        };
        let now = Instant::now();
        let frame = VideoFrame {
            data: vec![1; SENDER_MAX_VIDEO_CHUNK_BYTES * 20],
            is_key_frame: true,
            capture_at: now,
            encode_started_at: now,
            encode_completed_at: now,
        };
        sender
            .send_frame(&tx, peer, &mut cipher, 1, 1, frame, now)
            .unwrap();
        assert_eq!(sender.pending.as_ref().unwrap().index, 8);
        sender.send_audio(&tx, peer, &mut cipher, &[0; 8]).unwrap();
        let mut packet = [0; 1200];
        for _ in 0..9 {
            rx.recv(&mut packet).unwrap();
        }
        let size = rx.recv(&mut packet).unwrap();
        assert_eq!(
            decipher
                .open_datagram(&packet[..size])
                .unwrap()
                .0
                .packet_type,
            PacketType::AudioFrame
        );
        assert!(sender.pending.is_some());
        #[cfg(target_os = "linux")]
        {
            let buffers: Vec<_> = sender.batch.iter().map(|packet| packet.as_ptr()).collect();
            sender.send_video_burst(&tx, peer, &mut cipher).unwrap();
            assert_eq!(
                buffers,
                sender
                    .batch
                    .iter()
                    .map(|packet| packet.as_ptr())
                    .collect::<Vec<_>>()
            );
        }
        let smaller = UdpSender {
            send_capacity: 1200,
            bitrate: 100_000_000,
            ..Default::default()
        };
        assert!(smaller.burst_packets() < sender.burst_packets());
        assert!(smaller.burst_interval(1200) > sender.burst_interval(1200));
    }

    #[test]
    fn sender_reuses_video_packet_buffers() {
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let peer = rx.local_addr().unwrap();
        let mut cipher =
            DatagramCipher::derive(&[0x31; 32], &[0x72; 16], Direction::HostToClient).unwrap();
        let mut receiver =
            DatagramCipher::derive(&[0x31; 32], &[0x72; 16], Direction::HostToClient).unwrap();
        let mut sender = UdpSender::default();
        let expected: Vec<_> = (0..MAX_VIDEO_CHUNK_BYTES + 7)
            .map(|index| (index % 251) as u8)
            .collect();
        let now = Instant::now();
        let frame = || VideoFrame {
            data: expected.clone(),
            is_key_frame: true,
            capture_at: now,
            encode_started_at: now,
            encode_completed_at: now,
        };
        sender
            .send_frame(&tx, peer, &mut cipher, 320, 180, frame(), now)
            .unwrap();
        let mut packet = [0; 65536];
        for _ in 0..4 {
            rx.recv(&mut packet).unwrap();
        }
        let input = frame();
        let (result, count) = crate::test_alloc::allocations(|| {
            sender.send_frame(&tx, peer, &mut cipher, 320, 180, input, now)
        });
        result.unwrap();
        let mut chunks = std::collections::BTreeMap::new();
        let mut sequences = std::collections::BTreeSet::new();
        let mut saw_header = false;
        let mut saw_timing = false;
        for _ in 0..4 {
            let size = rx.recv(&mut packet).unwrap();
            let (header, payload) = receiver.open_datagram(&packet[..size]).unwrap();
            sequences.insert(header.sequence);
            match header.packet_type {
                PacketType::FrameHeader => {
                    let frame = FrameHeader::decode(&payload).unwrap();
                    assert_eq!((frame.frame_id, frame.width, frame.height), (2, 320, 180));
                    assert_eq!(frame.total_chunks, 2);
                    assert_eq!(frame.total_size as usize, expected.len());
                    saw_header = true;
                }
                PacketType::FrameChunk => {
                    let chunk = FrameChunk::decode(&payload).unwrap();
                    assert_eq!(chunk.frame_id, 2);
                    chunks.insert(chunk.chunk_index, chunk.data);
                }
                PacketType::Ping => {
                    assert!(!payload.is_empty());
                    saw_timing = true;
                }
                other => panic!("unexpected packet type {other:?}"),
            }
        }
        assert!(saw_header && saw_timing);
        assert_eq!(sequences.into_iter().collect::<Vec<_>>(), [5, 6, 7, 8]);
        assert_eq!(chunks.into_values().flatten().collect::<Vec<_>>(), expected);
        eprintln!("warm two-chunk video sender allocations: {count}");
        assert!(count <= 2, "fragment and datagram buffers must be reused");
    }

    #[test]
    fn sender_reuses_audio_packet_buffers() {
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let peer = rx.local_addr().unwrap();
        let mut cipher =
            DatagramCipher::derive(&[0x41; 32], &[0x82; 16], Direction::HostToClient).unwrap();
        let mut receiver =
            DatagramCipher::derive(&[0x41; 32], &[0x82; 16], Direction::HostToClient).unwrap();
        let mut sender = UdpSender::default();
        let expected = vec![0x29; MAX_AUDIO_FRAGMENT_BYTES + 9];
        sender
            .send_audio(&tx, peer, &mut cipher, &expected)
            .unwrap();
        let mut packet = [0; 65536];
        for _ in 0..2 {
            rx.recv(&mut packet).unwrap();
        }
        let (result, count) =
            crate::test_alloc::allocations(|| sender.send_audio(&tx, peer, &mut cipher, &expected));
        result.unwrap();
        let mut chunks = std::collections::BTreeMap::new();
        for _ in 0..2 {
            let size = rx.recv(&mut packet).unwrap();
            let (header, payload) = receiver.open_datagram(&packet[..size]).unwrap();
            assert_eq!(header.packet_type, PacketType::AudioFrame);
            let fragment = AudioFragment::decode(&payload).unwrap();
            assert_eq!(fragment.header.frame_id, 2);
            assert_eq!(fragment.header.fragment_count, 2);
            chunks.insert(fragment.header.fragment_index, fragment.data);
        }
        assert_eq!(chunks.into_values().flatten().collect::<Vec<_>>(), expected);
        let sequence = sender.sequence;
        sender.send_audio(&tx, peer, &mut cipher, &[]).unwrap();
        assert_eq!(sender.sequence, sequence);
        assert_eq!(sender.audio_frame_id, 2);
        eprintln!("warm two-fragment audio sender allocations: {count}");
        assert_eq!(
            count, 0,
            "audio fragment and datagram buffers must be reused"
        );
    }

    #[test]
    fn bootstrap_kdf_runs_once_per_unchanged_pairing_window() {
        let directory = tempdir().unwrap();
        let (consent, _) = mpsc::channel();
        let config = test_config(
            PairingStore::new(directory.path().join("keys.json")),
            consent,
        );
        BOOTSTRAP_DERIVATIONS.with(|count| count.set(0));
        let started = Instant::now();
        let server = HostServer::bind_synthetic(config, 0).unwrap();
        let first = server.current_psks().unwrap();
        for _ in 1..8 {
            assert_eq!(server.current_psks().unwrap(), first);
        }
        let derivations = BOOTSTRAP_DERIVATIONS.with(|count| count.get());
        eprintln!("unchanged pairing window: {derivations} PBKDF2 derivations for 8 accepts, elapsed {:?}", started.elapsed());
        assert_eq!(
            derivations, 1,
            "unchanged PIN repeats 600000-round PBKDF2 per accept"
        );
    }

    #[test]
    fn cached_bootstrap_obeys_lockout_expiry_and_pairing_revocation() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        store
            .save(PairingRecord {
                id: "stored".into(),
                name: "stored".into(),
                key: [3; 32],
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent, _) = mpsc::channel();
        let mut server =
            HostServer::bind_synthetic(test_config(store.clone(), consent), 0).unwrap();
        assert_eq!(server.current_psks().unwrap().len(), 2);
        for _ in 0..5 {
            server
                .lockout
                .lock()
                .unwrap()
                .record_failure(Instant::now());
        }
        let paired = server.current_psks().unwrap();
        assert_eq!(paired.len(), 1);
        assert_eq!(paired[0].identity(), "maho-p1.stored");
        server.lockout.lock().unwrap().begin_pairing();
        server.pairing_deadline = Some(Instant::now());
        assert_eq!(server.current_psks().unwrap(), paired);
        store.revoke("stored").unwrap();
        let disabled = server.current_psks().unwrap();
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].identity(), "maho-disabled");
    }

    #[test]
    fn host_default_store_path_and_current_psks_isolation() {
        let default_path = PairingStore::default_path().unwrap();
        assert_eq!(
            default_path.file_name().and_then(|n| n.to_str()),
            Some("host-authorizations.json"),
            "Host default path must be host-authorizations.json"
        );

        let directory = tempdir().unwrap();
        // Place a legacy pairing-keys.json in the directory
        let legacy_file = directory.path().join("pairing-keys.json");
        fs::write(
            &legacy_file,
            br#"[{"id":"legacy-peer-1","name":"Legacy","key":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=","addedAt":0.0}]"#,
        )
        .unwrap();

        // Host store pointed to host-authorizations.json in that directory must NOT import legacy-peer-1
        let host_file = directory.path().join("host-authorizations.json");
        let store = PairingStore::new(&host_file);
        assert!(store.load_all().unwrap().is_empty());

        let (consent, _) = mpsc::channel();
        let mut config = test_config(store.clone(), consent);
        config.bootstrap_pin = None;
        config.pairing_window = Duration::from_secs(0);
        let server = HostServer::bind_synthetic(config, 0).unwrap();
        let psks = server.current_psks().unwrap();
        // Since no bootstrap PIN and store is empty, psks only has the disabled fallback psk, never legacy-peer-1
        assert!(
            psks.iter().all(|p| p.identity() != "maho-p1.legacy-peer-1"),
            "Host current_psks must never contain legacy pairing records"
        );

        // Explicitly saved authorization in host store DOES appear in current_psks
        store
            .save(PairingRecord {
                id: "approved-inbound".into(),
                name: "Approved".into(),
                key: [7; 32],
                added_at_unix_ms: 1000,
            })
            .unwrap();
        let updated_psks = server.current_psks().unwrap();
        assert!(
            updated_psks
                .iter()
                .any(|p| p.identity() == "maho-p1.approved-inbound"),
            "Host current_psks must contain explicitly approved inbound authorization"
        );
    }

    #[cfg(target_os = "linux")]
    pub(super) fn admission_input(server: &HostServer) -> io::Result<LinuxInputInjector> {
        if let Some(input) = server.prepared_admission_input.lock().unwrap().take() {
            return Ok(input);
        }
        let geometry = probe_linux_input_geometry(server.config.output_name.as_deref())
            .unwrap_or_else(|| {
                crate::inject_linux::OutputGeometry::single_output(
                    server.config.display.pixel_width,
                    server.config.display.pixel_height,
                )
            });
        LinuxInputInjector::new(geometry)
    }

    fn prepare_admission_input(_server: &mut HostServer) {
        // Real uinput device registration contends across parallel tests. It is
        // a fixture prerequisite, not the TLS/consent wait being measured here.
        // Keep production construction and the single admission deadline intact.
        #[cfg(target_os = "linux")]
        {
            let started = Instant::now();
            let input = admission_input(_server).unwrap();
            *_server.prepared_admission_input.get_mut().unwrap() = Some(input);
            eprintln!("admission input prerequisite: {:?}", started.elapsed());
        }
    }

    fn pairing_deadline_scenario(approve: bool) {
        // Given: costly prerequisites are ready before the admission budget.
        let setup_started = Instant::now();
        let directory = tempdir().unwrap();
        let (consent, prompts) = mpsc::channel();
        let config = test_config(
            PairingStore::new(directory.path().join("keys.json")),
            consent,
        );
        let mut server = HostServer::bind_synthetic(config, 0).unwrap();
        prepare_admission_input(&mut server);
        let client = TlsPskClient::new(PskIdentity::bootstrap("12345678").unwrap()).unwrap();
        let tls = TlsPskServer::new(server.current_psks().unwrap()).unwrap();
        eprintln!(
            "pairing prerequisites (including both KDFs): {:?}",
            setup_started.elapsed()
        );
        let addr = server.tcp_addr().unwrap();
        let (deadline_tx, deadline_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let (socket, peer) = server.tcp_listener.accept().unwrap();
            let deadline = Instant::now() + Duration::from_millis(200);
            deadline_tx.send(deadline).unwrap();
            socket.set_nodelay(true).unwrap();
            let result = tls
                .accept_stream_until(socket, deadline)
                .map_err(SessionError::Tls)
                .and_then(|stream| {
                    let watch = SessionWatch::new(server.session_stall_timeout);
                    server.handle_connection(
                        stream,
                        peer,
                        deadline,
                        None,
                        &watch,
                        &PipelineSlot::default(),
                    )
                });
            done_tx.send((result, Instant::now())).unwrap();
        });
        let mut tcp = client.connect(addr).unwrap();
        let deadline = deadline_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        send_tcp_packet(
            &mut tcp,
            PacketType::PairingRequest,
            &PairingRequest {
                name: "deadline".into(),
            }
            .encode()
            .unwrap(),
        )
        .unwrap();
        let prompt = prompts
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|error| {
                panic!(
                    "consent channel failed: {error:?}; host result: {:?}",
                    done_rx.try_recv()
                )
            });
        // When: actual consent delivery (and, if approved, a wire grant) proves
        // entry to the state under test. An earlier timeout is never success.
        eprintln!(
            "consent entered with {:?} remaining",
            deadline.checked_duration_since(Instant::now())
        );
        let retained_prompt = if approve {
            prompt.approve();
            let packet = tcp.read_frame().unwrap();
            assert_eq!(
                split_packet(&packet).unwrap().0.packet_type,
                PacketType::PairingGrant
            );
            eprintln!(
                "grant received with {:?} remaining",
                deadline.checked_duration_since(Instant::now())
            );
            None
        } else {
            Some(prompt)
        };
        let completion = done_rx.recv_timeout(Duration::from_secs(2));
        let _ = tcp
            .ssl_stream()
            .get_ref()
            .shutdown(std::net::Shutdown::Both);
        drop(tcp);
        drop(retained_prompt);
        worker.join().unwrap();
        // Then: the original absolute deadline ends this specific state while
        // the client (and pending consent sender) are still open.
        let (result, ended_at) = completion.expect("preauth deadline must release the host");
        assert!(ended_at >= deadline, "admission ended before its deadline");
        eprintln!(
            "admission completed {:?} after deadline: {result:?}",
            ended_at.duration_since(deadline)
        );
        if approve {
            assert!(
                matches!(result, Err(SessionError::Io(error)) if error.kind() == io::ErrorKind::TimedOut)
            );
        } else {
            assert!(matches!(result, Err(SessionError::ConsentTimeout)));
        }
    }

    #[test]
    fn preauth_deadline_bounds_pending_consent() {
        pairing_deadline_scenario(false);
    }

    #[test]
    fn preauth_deadline_bounds_granted_client_without_handshake() {
        pairing_deadline_scenario(true);
    }

    #[test]
    fn preauth_deadline_releases_idle_client_before_peer_disconnect() {
        // Given: a paired TLS client that sends no application handshake.
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        store
            .save(PairingRecord {
                id: "deadline-client".into(),
                name: "deadline".into(),
                key: [7; 32],
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent, _) = mpsc::channel();
        let mut config = test_config(store, consent);
        config.bootstrap_pin = None;
        let mut server = HostServer::bind_synthetic(config, 0).unwrap();
        prepare_admission_input(&mut server);
        let tls = TlsPskServer::new(server.current_psks().unwrap()).unwrap();
        let addr = server.tcp_addr().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let (socket, peer) = server.tcp_listener.accept().unwrap();
            socket.set_nodelay(true).unwrap();
            let stream = tls
                .accept_stream_until(socket, Instant::now() + Duration::from_secs(2))
                .unwrap();
            let deadline = Instant::now() + Duration::from_millis(50);
            ready_tx.send(deadline).unwrap();
            let watch = SessionWatch::new(server.session_stall_timeout);
            let result = server.handle_connection(
                stream,
                peer,
                deadline,
                None,
                &watch,
                &PipelineSlot::default(),
            );
            done_tx.send((result, Instant::now())).unwrap();
        });
        let client =
            TlsPskClient::new(PskIdentity::pairing("deadline-client", &[7; 32]).unwrap()).unwrap();
        let tcp = client.connect(addr).unwrap();
        let deadline = ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // When: the total application-handshake budget expires with the peer open.
        let completion = done_rx.recv_timeout(Duration::from_secs(2));
        let ended_before_peer = completion.is_ok();
        let shutdown = tcp
            .ssl_stream()
            .get_ref()
            .shutdown(std::net::Shutdown::Both);
        drop(tcp);
        worker.join().unwrap();
        assert!(shutdown.is_ok() || shutdown.unwrap_err().kind() == io::ErrorKind::NotConnected);
        // Then: the host, not peer disconnect, ends admission with an error.
        assert!(
            ended_before_peer,
            "idle preauth client monopolized host admission"
        );
        let (result, ended_at) = completion.unwrap();
        assert!(ended_at >= deadline, "admission ended before its deadline");
        assert!(
            matches!(result, Err(SessionError::Io(error)) if error.kind() == io::ErrorKind::TimedOut)
        );
    }

    #[test]
    fn admission_deadline_releases_silent_tls_then_accepts_paired_client() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        store
            .save(PairingRecord {
                id: "deadline-client".into(),
                name: "deadline".into(),
                key: [7; 32],
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent, _) = mpsc::channel();
        let mut config = test_config(store, consent);
        config.bootstrap_pin = None;
        let mut server = HostServer::bind_synthetic(config, 0).unwrap();
        server.preauth_timeout = Duration::from_millis(100);
        prepare_admission_input(&mut server);
        let addr = server.tcp_addr().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            done_tx.send(server.serve_next()).unwrap();
            done_tx.send(server.serve_next()).unwrap();
        });
        let silent = TcpStream::connect(addr).unwrap();
        let first = done_rx.recv_timeout(Duration::from_secs(1));
        let ended_before_peer = first.is_ok();
        let _ = silent.shutdown(std::net::Shutdown::Both);
        drop(silent);
        let first = match first {
            Ok(result) => result,
            Err(_) => done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        };
        let client =
            TlsPskClient::new(PskIdentity::pairing("deadline-client", &[7; 32]).unwrap()).unwrap();
        let socket = TcpStream::connect(addr).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut tcp = client.connect_stream(socket).unwrap();
        let handshake = Handshake {
            name: "deadline".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: "deadline-client".into(),
            session_salt: [9; 16],
        };
        send_tcp_packet(
            &mut tcp,
            PacketType::Handshake,
            &handshake.encode().unwrap(),
        )
        .unwrap();
        assert_eq!(
            split_packet(&tcp.read_frame().unwrap())
                .unwrap()
                .0
                .packet_type,
            PacketType::HandshakeAck
        );
        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        let second = done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        assert!(
            ended_before_peer,
            "silent TLS peer monopolized serial accept"
        );
        assert!(
            matches!(first, Err(SessionError::Tls(TlsPskError::Io(error))) if error.kind() == io::ErrorKind::TimedOut)
        );
        assert!(second.is_ok());
    }
    use ferryx_rd_net::{PskIdentity, TlsPskClient};
    use tempfile::tempdir;

    #[test]
    fn input_injection_preserves_behavior_with_trace_only_metadata() {
        use tracing_subscriber::prelude::*;
        struct Levels(Arc<Mutex<Vec<tracing::Level>>>);
        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Levels {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _: tracing_subscriber::layer::Context<'_, S>,
            ) {
                self.0.lock().unwrap().push(*event.metadata().level());
            }
        }
        let levels = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(Levels(levels.clone()));
        let event = InputEvent::decode(&[0; InputEvent::SIZE]).unwrap();
        let mut injected = Vec::new();
        tracing::subscriber::with_default(subscriber, || {
            inject_input(&event, |actual| {
                injected.push(actual.encode().unwrap());
                Ok::<(), io::Error>(())
            });
        });
        assert_eq!(injected, vec![event.encode().unwrap()]);
        assert_eq!(*levels.lock().unwrap(), vec![tracing::Level::TRACE]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn capture_overload_drops_frame_but_disconnection_stops_bridge() {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(crate::encode_vt::Command::ForceKeyFrame).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let frame = || CaptureFrame {
            width: 2,
            height: 2,
            bytes_per_row: 8,
            bgra: vec![0; 16],
            captured_at: Instant::now(),
        };
        let worker = thread::spawn(move || {
            let keep_running = submit_capture_frame(&tx, frame());
            done_tx.send(keep_running).unwrap();
            tx
        });
        let completed = done_rx.recv_timeout(Duration::from_secs(2));
        // Release the stalled baseline before asserting, so RED leaves no worker behind.
        drop(rx);
        let tx = worker.join().unwrap();
        assert_eq!(completed, Ok(true));
        assert!(!submit_capture_frame(&tx, frame()));
    }

    struct LifecycleSource {
        full: mpsc::Sender<()>,
        stopped: mpsc::Sender<()>,
        sender_exit: mpsc::Sender<()>,
    }

    struct LifecycleHandle {
        producer: Option<thread::JoinHandle<()>>,
        exited: Receiver<()>,
        stopped: mpsc::Sender<()>,
    }

    impl MediaSource for LifecycleSource {
        fn sender_exited(&self) {
            self.sender_exit.send(()).unwrap();
        }
        fn start_with_capabilities(
            &self,
            sender: SyncSender<MediaEvent>,
            _peer: Capabilities,
        ) -> Result<Box<dyn MediaHandle>, SessionError> {
            let full = self.full.clone();
            let (exit_tx, exited) = mpsc::channel();
            let producer = thread::spawn(move || {
                let now = Instant::now();
                let frame = VideoFrame {
                    data: vec![0, 0, 0, 1, 0x26],
                    is_key_frame: true,
                    capture_at: now,
                    encode_started_at: now,
                    encode_completed_at: now,
                };
                for _ in 0..SESSION_MEDIA_QUEUE_DEPTH {
                    sender.send(MediaEvent::Video(frame.clone())).unwrap();
                }
                assert!(matches!(
                    sender.try_send(MediaEvent::Video(frame.clone())),
                    Err(mpsc::TrySendError::Full(_))
                ));
                full.send(()).unwrap();
                while sender.send(MediaEvent::Video(frame.clone())).is_ok() {}
                exit_tx.send(()).unwrap();
            });
            Ok(Box::new(LifecycleHandle {
                producer: Some(producer),
                exited,
                stopped: self.stopped.clone(),
            }))
        }
    }

    impl MediaHandle for LifecycleHandle {
        fn force_key_frame(&self) -> Result<(), SessionError> {
            Ok(())
        }
        fn update_bitrate(&self, _: u32) -> Result<(), SessionError> {
            Ok(())
        }
        fn stop(&mut self) {
            self.exited
                .recv_timeout(Duration::from_secs(2))
                .expect("producer exit before join");
            self.producer.take().unwrap().join().unwrap();
            self.stopped.send(()).unwrap();
        }
    }

    fn lifecycle_scenario(malformed: bool) {
        // Given: real paired TLS and an owned producer filling the actual media queue.
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [3; 32];
        store
            .save(PairingRecord {
                id: "lifecycle".into(),
                name: "fixture".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent, _) = mpsc::channel();
        let (full_tx, full_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (sender_exit, sender_exited) = mpsc::channel();
        let server = HostServer::bind_with_media(
            test_config(store, consent),
            Arc::new(LifecycleSource {
                full: full_tx,
                stopped: stop_tx,
                sender_exit,
            }),
        )
        .unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let udp_addr = server.udp_addr().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let (tcp, peer) = server.tcp_listener.accept().unwrap();
            let tls = TlsPskServer::new(server.current_psks().unwrap()).unwrap();
            let result = server.handle_connection(
                tls.accept_stream(tcp).unwrap(),
                peer,
                Instant::now() + server.preauth_timeout,
                None,
                &SessionWatch::new(server.session_stall_timeout),
                &PipelineSlot::default(),
            );
            done_tx.send(result).unwrap();
        });
        let client = TlsPskClient::new(PskIdentity::pairing("lifecycle", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        tcp.ssl_stream_mut()
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let handshake = Handshake {
            name: "fixture".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: "lifecycle".into(),
            session_salt: [5; 16],
        };
        send_tcp_packet(
            &mut tcp,
            PacketType::Handshake,
            &handshake.encode().unwrap(),
        )
        .unwrap();
        assert_eq!(
            split_packet(&tcp.read_frame().unwrap())
                .unwrap()
                .0
                .packet_type,
            PacketType::HandshakeAck
        );
        full_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        // When: disconnect before discovery, or corrupt control after real UDP output.
        if malformed {
            let mut c2h = DatagramCipher::derive(&key, &[5; 16], Direction::ClientToHost).unwrap();
            let reg = c2h
                .seal_datagram(&PacketHeader::new(PacketType::Ping, 0, 0, 0), &[])
                .unwrap();
            udp.send_to(&reg, udp_addr).unwrap();
            let mut buffer = [0; 2048];
            let (length, _) = udp.recv_from(&mut buffer).unwrap();
            let mut cipher =
                DatagramCipher::derive(&key, &[5; 16], Direction::HostToClient).unwrap();
            assert_eq!(
                cipher
                    .open_datagram(&buffer[..length])
                    .unwrap()
                    .0
                    .packet_type,
                PacketType::FrameHeader
            );
            send_tcp_packet(&mut tcp, PacketType::Control, &[]).unwrap();
        } else {
            send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        }
        // Then: producer is disconnected and joined before the session completes.
        let stopped = stop_rx.recv_timeout(Duration::from_secs(3));
        let done = done_rx.recv_timeout(Duration::from_secs(3));
        let joined = worker.join();
        assert!(
            stopped.is_ok(),
            "owned producer must be stopped and joined: {stopped:?}"
        );
        assert!(joined.is_ok());
        if malformed {
            sender_exited.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        let result = done.unwrap();
        if malformed {
            assert!(matches!(result, Err(SessionError::Codec(_))));
        } else {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn disconnect_before_udp_unblocks_full_media_queue() {
        lifecycle_scenario(false);
    }

    #[test]
    fn protocol_error_stops_all_media_workers() {
        lifecycle_scenario(true);
    }

    fn test_config(store: PairingStore, consent_sender: mpsc::Sender<ConsentPrompt>) -> HostConfig {
        HostConfig {
            tcp_addr: "127.0.0.1:0".parse().unwrap(),
            udp_addr: "127.0.0.1:0".parse().unwrap(),
            bootstrap_pin: Some("12345678".into()),
            pairing_window: Duration::from_secs(5),
            pairing_store: store,
            host_name: "test-host".into(),
            display: DisplayInfo {
                desktop_x: 0,
                desktop_y: 0,
                logical_width: 640,
                logical_height: 360,
                pixel_width: 640,
                pixel_height: 360,
                scale_factor_milli: 1_000,
            },
            frames_per_second: 60,
            bitrate: 12_000_000,
            capture_audio: false,
            output_name: None,
            consent_sender: Some(consent_sender),
            allow_input: Arc::new(AtomicBool::new(true)),
            allow_clipboard: Arc::new(AtomicBool::new(true)),
        }
    }

    fn decode_tcp_packet(packet: &[u8]) -> (PacketHeader, &[u8]) {
        split_packet(packet).unwrap()
    }

    #[test]
    fn resolve_output_target_precedence_and_strict_explicit_matching() {
        let monitors_focused: serde_json::Value = serde_json::json!([
            {"name": "DP-1", "focused": false},
            {"name": "HDMI-A-1", "focused": true}
        ]);
        let monitors_none_focused: serde_json::Value = serde_json::json!([
            {"name": "DP-1", "focused": false},
            {"name": "HDMI-A-1", "focused": false}
        ]);
        let empty_monitors: serde_json::Value = serde_json::json!([]);

        // 1. Explicit CLI argument takes highest precedence over env and compositor
        assert_eq!(
            resolve_output_target(
                Some("DP-2".to_string()),
                Some("HDMI-A-1".to_string()),
                Some(&monitors_focused),
            ),
            Some("DP-2".to_string())
        );

        // 2. Whitespace-only CLI argument falls through to env
        assert_eq!(
            resolve_output_target(
                Some("   ".to_string()),
                Some("HDMI-A-1".to_string()),
                Some(&monitors_focused),
            ),
            Some("HDMI-A-1".to_string())
        );

        // 3. Explicit env variable takes precedence over compositor when CLI is absent
        assert_eq!(
            resolve_output_target(None, Some("HDMI-A-1".to_string()), Some(&monitors_focused),),
            Some("HDMI-A-1".to_string())
        );

        // 4. Explicit non-existent target in env is strictly preserved (never silently redirected)
        assert_eq!(
            resolve_output_target(
                None,
                Some("NON_EXISTENT_MONITOR".to_string()),
                Some(&monitors_focused),
            ),
            Some("NON_EXISTENT_MONITOR".to_string())
        );

        // 5. Auto-detect with focused monitor selects the focused monitor
        assert_eq!(
            resolve_output_target(None, None, Some(&monitors_focused)),
            Some("HDMI-A-1".to_string())
        );

        // 6. Auto-detect with no focused monitor falls back to first named monitor
        assert_eq!(
            resolve_output_target(None, None, Some(&monitors_none_focused)),
            Some("DP-1".to_string())
        );

        // 7. Auto-detect with empty monitors array returns None
        assert_eq!(
            resolve_output_target(None, None, Some(&empty_monitors)),
            None
        );

        // 8. Auto-detect with no compositor data returns None
        assert_eq!(resolve_output_target(None, None, None), None);
    }

    #[test]
    fn select_focused_output_finds_focused_or_falls_back_to_first() {
        let json_with_focused: serde_json::Value = serde_json::json!([
            {"name": "DP-1", "focused": false},
            {"name": "HDMI-A-1", "focused": true}
        ]);
        assert_eq!(
            select_focused_output(&json_with_focused),
            Some("HDMI-A-1".to_string())
        );

        let json_no_focused: serde_json::Value = serde_json::json!([
            {"name": "HDMI-A-1", "focused": false},
            {"name": "DP-1", "focused": false}
        ]);
        assert_eq!(
            select_focused_output(&json_no_focused),
            Some("HDMI-A-1".to_string())
        );

        let empty_array: serde_json::Value = serde_json::json!([]);
        assert_eq!(select_focused_output(&empty_array), None);

        let not_an_array: serde_json::Value = serde_json::json!({"name": "HDMI-A-1"});
        assert_eq!(select_focused_output(&not_an_array), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_hyprland_signature_falls_back_to_rediscovered_instance() {
        // A daemon that outlives a Hyprland restart keeps the dead signature in
        // its environment. `hyprctl -i <stale>` fails, and treating that as "no
        // compositor" degraded input geometry to a single-output fallback, so
        // the pointer was scaled across the wrong desktop width.
        let stale = "efb5_1790252082_167242086";
        let live = "efb5_1790252620_1733994707";
        let older = "efb5_1789367143_909438767";
        let candidates = vec![live.to_owned(), stale.to_owned(), older.to_owned()];
        let monitors = serde_json::json!([{"name": "HDMI-A-2"}]);
        let mut probed: Vec<Option<String>> = Vec::new();

        let resolved = hyprland_monitors_from_candidates(Some(stale), &candidates, |sig| {
            probed.push(sig.map(str::to_owned));
            (sig == Some(live)).then(|| monitors.clone())
        });

        assert_eq!(resolved, Some(monitors));
        assert_eq!(
            probed,
            vec![Some(stale.to_owned()), Some(live.to_owned())],
            "a stale signature must be retried against the newest live instance"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn live_hyprland_signature_is_used_without_rediscovery() {
        let live = "efb5_1790252620_1733994707";
        let candidates = vec![live.to_owned(), "other".to_owned()];
        let monitors = serde_json::json!([{"name": "HDMI-A-2"}]);
        let mut probes = 0_u32;

        let resolved = hyprland_monitors_from_candidates(Some(live), &candidates, |_| {
            probes += 1;
            Some(monitors.clone())
        });

        assert_eq!(resolved, Some(monitors));
        assert_eq!(probes, 1, "a live signature must be probed exactly once");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn empty_or_absent_signature_uses_candidates_then_bare_probe() {
        let monitors = serde_json::json!([{"name": "HDMI-A-2"}]);

        // An empty inherited signature is not a candidate at all.
        let candidates = vec!["live".to_owned()];
        let mut probed: Vec<Option<String>> = Vec::new();
        let resolved = hyprland_monitors_from_candidates(Some(""), &candidates, |sig| {
            probed.push(sig.map(str::to_owned));
            (sig == Some("live")).then(|| monitors.clone())
        });
        assert_eq!(resolved, Some(monitors));
        assert_eq!(probed, vec![Some("live".to_owned())]);

        // With nothing discoverable the bare probe still runs; the caller then
        // falls back to the single-output geometry it already owned.
        let mut bare: Vec<Option<String>> = Vec::new();
        let resolved = hyprland_monitors_from_candidates(None, &[], |sig| {
            bare.push(sig.map(str::to_owned));
            None
        });
        assert_eq!(resolved, None);
        assert_eq!(bare, vec![None]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_signature_is_not_reprobed_as_a_candidate() {
        // The stale signature is already known-dead, so discovery must not
        // spend a second `hyprctl` round trip on it.
        let stale = "efb5_1790252082_167242086";
        let candidates = vec![stale.to_owned(), "live".to_owned()];
        let mut probed: Vec<Option<String>> = Vec::new();

        let resolved = hyprland_monitors_from_candidates(Some(stale), &candidates, |sig| {
            probed.push(sig.map(str::to_owned));
            None
        });

        assert_eq!(resolved, None);
        assert_eq!(
            probed,
            vec![Some(stale.to_owned()), Some("live".to_owned()), None],
            "the stale signature must not be retried during discovery"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn regression_multi_monitor_output_geometry_matches_desktop_topology() {
        // 1. Live Linux scenario from diagnosis:
        // HDMI-A-2: 3840x1600 at (0, 0), HEADLESS-1: 2560x1440 at (3840, 0).
        // Total desktop bounding box: 6400x1600.
        let monitors = serde_json::json!([
            {
                "name": "HDMI-A-2",
                "x": 0,
                "y": 0,
                "width": 3840,
                "height": 1600,
                "scale": 1.0,
                "transform": 0,
                "focused": true,
                "disabled": false
            },
            {
                "name": "HEADLESS-1",
                "x": 3840,
                "y": 0,
                "width": 2560,
                "height": 1440,
                "scale": 1.0,
                "transform": 0,
                "focused": false,
                "disabled": false
            }
        ]);

        let hdmi =
            output_geometry_from_monitors(&monitors, Some("HDMI-A-2")).expect("HDMI-A-2 geometry");
        assert_eq!(hdmi.desktop_x, 0);
        assert_eq!(hdmi.desktop_y, 0);
        assert_eq!(hdmi.desktop_width, 6400);
        assert_eq!(hdmi.desktop_height, 1600);
        assert_eq!(hdmi.x, 0);
        assert_eq!(hdmi.y, 0);
        assert_eq!(hdmi.width, 3840);
        assert_eq!(hdmi.height, 1600);

        // Verification of coordinate mapping on the 6400x1600 desktop:
        // Right edge of HDMI-A-2 (x = 1.0) must map to pixel 3839 within the 6400 uinput range,
        // preventing the horizontal scaling bug where 3839 was scaled across 6400.
        let (mapped_x, _) = crate::inject_linux::map_normalized_to_output(1.0, 0.5, hdmi);
        assert_eq!(mapped_x, 3839);

        // Selecting HEADLESS-1 on the same topology:
        let headless = output_geometry_from_monitors(&monitors, Some("HEADLESS-1"))
            .expect("HEADLESS-1 geometry");
        assert_eq!(headless.desktop_width, 6400);
        assert_eq!(headless.desktop_height, 1600);
        assert_eq!(headless.x, 3840);
        assert_eq!(headless.y, 0);
        assert_eq!(headless.width, 2560);
        assert_eq!(headless.height, 1440);

        // 2. Omarchy live compositor arrangement:
        // HEADLESS-1 at (0, 0) 2560x1440, HDMI-A-2 at (2560, 0) 3840x1600.
        let omarchy_monitors = serde_json::json!([
            {
                "name": "HEADLESS-1",
                "x": 0,
                "y": 0,
                "width": 2560,
                "height": 1440,
                "scale": 1.0,
                "transform": 0,
                "focused": false,
                "disabled": false
            },
            {
                "name": "HDMI-A-2",
                "x": 2560,
                "y": 0,
                "width": 3840,
                "height": 1600,
                "scale": 1.0,
                "transform": 0,
                "focused": true,
                "disabled": false
            }
        ]);

        let omarchy_hdmi = output_geometry_from_monitors(&omarchy_monitors, Some("HDMI-A-2"))
            .expect("Omarchy HDMI-A-2 geometry");
        assert_eq!(omarchy_hdmi.desktop_width, 6400);
        assert_eq!(omarchy_hdmi.desktop_height, 1600);
        assert_eq!(omarchy_hdmi.x, 2560);
        assert_eq!(omarchy_hdmi.y, 0);
        assert_eq!(omarchy_hdmi.width, 3840);
        assert_eq!(omarchy_hdmi.height, 1600);

        let (left_x, _) = crate::inject_linux::map_normalized_to_output(0.0, 1.0, omarchy_hdmi);
        let (right_x, _) = crate::inject_linux::map_normalized_to_output(1.0, 1.0, omarchy_hdmi);
        assert_eq!(left_x, 2560);
        assert_eq!(right_x, 6399);

        // 3. Fallback to focused or first monitor when target_output_name is None:
        let auto_focused =
            output_geometry_from_monitors(&omarchy_monitors, None).expect("auto-focused geometry");
        assert_eq!(auto_focused, omarchy_hdmi);

        // 4. Negative desktop origin:
        let negative_monitors = serde_json::json!([
            {
                "name": "DP-1",
                "x": -1920,
                "y": -1080,
                "width": 1920,
                "height": 1080,
                "scale": 1.0,
                "transform": 0,
                "disabled": false
            },
            {
                "name": "PRIMARY",
                "x": 0,
                "y": 0,
                "width": 2560,
                "height": 1440,
                "scale": 1.0,
                "transform": 0,
                "disabled": false
            }
        ]);
        let neg_geom = output_geometry_from_monitors(&negative_monitors, Some("DP-1"))
            .expect("negative origin geometry");
        assert_eq!(neg_geom.desktop_x, -1920);
        assert_eq!(neg_geom.desktop_y, -1080);
        assert_eq!(neg_geom.desktop_width, 4480);
        assert_eq!(neg_geom.desktop_height, 2520);
        assert_eq!(neg_geom.x, -1920);
        assert_eq!(neg_geom.y, -1080);

        // 5. Scaling and rotation transforms:
        let transformed_monitors = serde_json::json!([
            {
                "name": "SCALED-4K",
                "x": 0,
                "y": 0,
                "width": 3840,
                "height": 2160,
                "scale": 2.0,
                "transform": 0,
                "disabled": false
            },
            {
                "name": "ROTATED-PORTRAIT",
                "x": 1920,
                "y": 0,
                "width": 1920,
                "height": 1080,
                "scale": 1.0,
                "transform": 1,
                "disabled": false
            },
            {
                "name": "OFFLINE",
                "x": 9999,
                "y": 9999,
                "width": 1920,
                "height": 1080,
                "disabled": true
            }
        ]);
        let scaled = output_geometry_from_monitors(&transformed_monitors, Some("SCALED-4K"))
            .expect("scaled geometry");
        assert_eq!(scaled.width, 1920);
        assert_eq!(scaled.height, 1080);
        // Total desktop without disabled monitor:
        // SCALED-4K: (0,0) -> 1920x1080. ROTATED-PORTRAIT: (1920,0) -> 1080x1920.
        // desktop: x: 0..3000, y: 0..1920.
        assert_eq!(scaled.desktop_width, 3000);
        assert_eq!(scaled.desktop_height, 1920);

        // 6. Explicit target missing must strictly return None (never silently fallback):
        assert_eq!(
            output_geometry_from_monitors(&monitors, Some("NON_EXISTENT")),
            None,
            "missing explicit target must return None"
        );

        // 7. Invalid / empty monitors returns None:
        let empty = serde_json::json!([]);
        assert_eq!(output_geometry_from_monitors(&empty, None), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn deterministic_exact_target_selection_and_fallback_precedence() {
        let monitors = serde_json::json!([
            {
                "name": "DP-1",
                "x": 0,
                "y": 0,
                "width": 1920,
                "height": 1080,
                "scale": 1.0,
                "transform": 0,
                "focused": false,
                "disabled": false
            },
            {
                "name": "HDMI-A-1",
                "x": 1920,
                "y": 0,
                "width": 2560,
                "height": 1440,
                "scale": 1.0,
                "transform": 0,
                "focused": true,
                "disabled": false
            }
        ]);

        // Explicit target match selects exact target
        let exact_dp =
            output_geometry_from_monitors(&monitors, Some("DP-1")).expect("exact DP-1 selection");
        assert_eq!(exact_dp.x, 0);
        assert_eq!(exact_dp.width, 1920);

        let exact_hdmi = output_geometry_from_monitors(&monitors, Some("HDMI-A-1"))
            .expect("exact HDMI-A-1 selection");
        assert_eq!(exact_hdmi.x, 1920);
        assert_eq!(exact_hdmi.width, 2560);

        // Explicit target missing must strictly return None (must NOT silently fall back to focused HDMI-A-1)
        assert_eq!(
            output_geometry_from_monitors(&monitors, Some("NON_EXISTENT")),
            None
        );

        // Auto-detect with focused monitor selects the focused monitor
        let auto_focused =
            output_geometry_from_monitors(&monitors, None).expect("auto focused selection");
        assert_eq!(auto_focused.x, 1920);
        assert_eq!(auto_focused.width, 2560);

        // Auto-detect with no focused monitor falls back to first available monitor
        let monitors_none_focused = serde_json::json!([
            {
                "name": "DP-1",
                "x": 0,
                "y": 0,
                "width": 1920,
                "height": 1080,
                "scale": 1.0,
                "transform": 0,
                "focused": false,
                "disabled": false
            },
            {
                "name": "HDMI-A-1",
                "x": 1920,
                "y": 0,
                "width": 2560,
                "height": 1440,
                "scale": 1.0,
                "transform": 0,
                "focused": false,
                "disabled": false
            }
        ]);
        let auto_first = output_geometry_from_monitors(&monitors_none_focused, None)
            .expect("auto first selection");
        assert_eq!(auto_first.x, 0);
        assert_eq!(auto_first.width, 1920);
    }

    #[test]
    fn pre_auth_gating_refuses_input_and_control() {
        assert!(!SessionState::PreAuth.allows(PacketType::InputEvent));
        assert!(!SessionState::PreAuth.allows(PacketType::Control));
        assert!(SessionState::PreAuth.allows(PacketType::PairingRequest));
        assert!(SessionState::Authenticated.allows(PacketType::InputEvent));
    }

    #[test]
    fn lockout_disables_bootstrap_after_five_failures() {
        let now = Instant::now();
        let mut lockout = BootstrapLockout::default();
        lockout.begin_pairing();
        for attempt in 0..4 {
            assert!(!lockout.record_failure(now + Duration::from_secs(attempt)));
        }
        assert!(lockout.record_failure(now + Duration::from_secs(4)));
        assert!(!lockout.is_allowed(now + Duration::from_secs(5)));
    }

    #[test]
    fn direction_keys_arm_and_only_matching_direction_opens() {
        let key = [7_u8; 32];
        let salt = [9_u8; 16];
        let header = PacketHeader::new(PacketType::Ping, 1, 2, 0);
        let mut host_send = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let mut client_receive =
            DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let mut wrong = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
        let datagram = host_send.seal_datagram(&header, b"armed").unwrap();
        assert_eq!(client_receive.open_datagram(&datagram).unwrap().1, b"armed");
        assert!(wrong.open_datagram(&datagram).is_err());
    }

    #[test]
    fn pairing_store_mirrors_swift_json_and_is_mode_0600() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let record = PairingRecord {
            id: "id".into(),
            name: "client".into(),
            key: [3; 32],
            added_at_unix_ms: 1_700_000_000_000,
        };
        store.save(record.clone()).unwrap();
        assert_eq!(store.load("id").unwrap(), Some(record));
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert!(json[0]["key"].is_string());
        assert!(json[0]["addedAt"].is_number());
    }

    #[test]
    fn timestamp_stats_round_trip() {
        let stats = TimestampStats {
            frame_id: 7,
            capture_us: 10,
            encode_start_us: 20,
            encode_end_us: 30,
            send_us: 40,
        };
        assert_eq!(TimestampStats::decode(&stats.encode()), Some(stats));
    }

    #[test]
    fn scripted_loopback_pairs_arms_ciphers_and_receives_ten_timestamped_frames() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let (consent_tx, consent_rx) = mpsc::channel();
        let server = HostServer::bind_synthetic(test_config(store, consent_tx), 12).unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let udp_addr = server.udp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());
        let consent_thread = thread::spawn(move || {
            let prompt = consent_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(prompt.client_name, "scripted-client");
            prompt.approve();
        });

        let client = TlsPskClient::new(PskIdentity::bootstrap("12345678").unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let request = PairingRequest {
            name: "scripted-client".into(),
        };
        let mut packet = PacketHeader::new(PacketType::PairingRequest, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&request.encode().unwrap());
        tcp.write_frame(&packet).unwrap();
        let grant_packet = tcp.read_frame().unwrap();
        let (header, payload) = decode_tcp_packet(&grant_packet);
        assert_eq!(header.packet_type, PacketType::PairingGrant);
        let grant = PairingGrant::decode(payload).unwrap();

        let salt = [0x55_u8; 16];
        let handshake = Handshake {
            name: "scripted-client".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: grant.pairing_id,
            session_salt: salt,
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();
        let ack = tcp.read_frame().unwrap();
        assert_eq!(
            decode_tcp_packet(&ack).0.packet_type,
            PacketType::HandshakeAck
        );

        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut c2h = DatagramCipher::derive(&grant.key, &salt, Direction::ClientToHost).unwrap();
        let reg = c2h
            .seal_datagram(&PacketHeader::new(PacketType::Ping, 0, 0, 0), &[])
            .unwrap();
        udp.send_to(&reg, udp_addr).unwrap();
        let mut receive =
            DatagramCipher::derive(&grant.key, &salt, Direction::HostToClient).unwrap();
        let mut buffer = [0_u8; 2_048];
        let mut timestamps = Vec::new();
        while timestamps.len() < 10 {
            let (length, _) = udp.recv_from(&mut buffer).unwrap();
            let (header, payload) = receive.open_datagram(&buffer[..length]).unwrap();
            if header.packet_type == PacketType::Ping {
                if let Some(stats) = TimestampStats::decode(&payload) {
                    assert!(stats.capture_us <= stats.encode_start_us);
                    assert!(stats.encode_start_us <= stats.encode_end_us);
                    assert!(stats.encode_end_us <= stats.send_us);
                    timestamps.push(stats);
                }
            }
        }
        assert!(timestamps
            .windows(2)
            .all(|pair| pair[0].frame_id < pair[1].frame_id));
        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        consent_thread.join().unwrap();
        server_thread.join().unwrap();
    }

    struct BitrateRecordSource {
        bitrates: Arc<Mutex<Vec<u32>>>,
    }

    struct BitrateRecordHandle {
        bitrates: Arc<Mutex<Vec<u32>>>,
    }

    impl MediaSource for BitrateRecordSource {
        fn start_with_capabilities(
            &self,
            _sender: SyncSender<MediaEvent>,
            _peer: Capabilities,
        ) -> Result<Box<dyn MediaHandle>, SessionError> {
            Ok(Box::new(BitrateRecordHandle {
                bitrates: Arc::clone(&self.bitrates),
            }))
        }
    }

    impl MediaHandle for BitrateRecordHandle {
        fn force_key_frame(&self) -> Result<(), SessionError> {
            Ok(())
        }
        fn update_bitrate(&self, bitrate: u32) -> Result<(), SessionError> {
            self.bitrates.lock().unwrap().push(bitrate);
            Ok(())
        }
        fn stop(&mut self) {}
    }

    #[test]
    fn bitrate_adjust_updates_encoder_and_rejects_malformed_limits() {
        // Given: paired host server with tracked media handle.
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x5a; 32];
        store
            .save(PairingRecord {
                id: "bitrate-test".into(),
                name: "fixture".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let bitrates = Arc::new(Mutex::new(Vec::new()));
        let source = Arc::new(BitrateRecordSource {
            bitrates: Arc::clone(&bitrates),
        });
        let server = HostServer::bind_with_media(config, source).unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());

        let client =
            TlsPskClient::new(PskIdentity::pairing("bitrate-test", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let handshake = Handshake {
            name: "scripted-client".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: "bitrate-test".into(),
            session_salt: [0x33; 16],
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();
        let ack = tcp.read_frame().unwrap();
        assert_eq!(
            decode_tcp_packet(&ack).0.packet_type,
            PacketType::HandshakeAck
        );

        // When: client submits malformed non-positive and valid positive bitrates.
        send_tcp_control(
            &mut tcp,
            ControlMessage::BitrateAdjust(BitrateAdjust { target_bitrate: 0 }),
        )
        .unwrap();
        send_tcp_control(
            &mut tcp,
            ControlMessage::BitrateAdjust(BitrateAdjust {
                target_bitrate: -100,
            }),
        )
        .unwrap();
        send_tcp_control(
            &mut tcp,
            ControlMessage::BitrateAdjust(BitrateAdjust {
                target_bitrate: 4_000_000,
            }),
        )
        .unwrap();
        send_tcp_control(
            &mut tcp,
            ControlMessage::BitrateAdjust(BitrateAdjust {
                target_bitrate: 15_000_000,
            }),
        )
        .unwrap();
        send_tcp_control(&mut tcp, ControlMessage::Ping).unwrap();
        let pong = tcp.read_frame().unwrap();
        let (header, payload) = decode_tcp_packet(&pong);
        assert_eq!(header.packet_type, PacketType::Control);
        assert_eq!(
            ControlMessage::decode(payload).unwrap(),
            ControlMessage::Pong
        );

        // Then: host applied only positive bitrates to media encoder in exact order.
        assert_eq!(*bitrates.lock().unwrap(), vec![4_000_000, 15_000_000]);

        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        server_thread.join().unwrap();
    }

    #[test]
    fn stream_configuration_negotiates_valid_and_rejects_invalid_values() {
        // Given: paired host server with tracked media handle.
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x7c; 32];
        store
            .save(PairingRecord {
                id: "config-test".into(),
                name: "fixture".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let bitrates = Arc::new(Mutex::new(Vec::new()));
        let source = Arc::new(BitrateRecordSource {
            bitrates: Arc::clone(&bitrates),
        });
        let server = HostServer::bind_with_media(config, source).unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());

        let client = TlsPskClient::new(PskIdentity::pairing("config-test", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let handshake = Handshake {
            name: "scripted-client".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: "config-test".into(),
            session_salt: [0x88; 16],
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();
        let ack = tcp.read_frame().unwrap();
        assert_eq!(
            decode_tcp_packet(&ack).0.packet_type,
            PacketType::HandshakeAck
        );

        // When: client requests invalid dimensions.
        let invalid_dim = ferryx_rd_proto::StreamConfigurationRequest {
            request_id: 101,
            desired: ferryx_rd_proto::StreamConfiguration {
                width: 0,
                height: 1080,
                bitrate: 5_000_000,
                frames_per_second: 60,
            },
        };
        send_tcp_control(&mut tcp, ControlMessage::StreamConfigRequest(invalid_dim)).unwrap();
        let reject_frame = tcp.read_frame().unwrap();
        let (header, payload) = decode_tcp_packet(&reject_frame);
        assert_eq!(header.packet_type, PacketType::Control);
        match ControlMessage::decode(payload).unwrap() {
            ControlMessage::StreamConfigReject(rej) => {
                assert_eq!(rej.request_id, 101);
                assert_eq!(
                    rej.reason,
                    StreamConfigurationErrorCode::UnsupportedDimensions
                );
            }
            other => panic!("expected reject, got {other:?}"),
        }

        // When: client requests invalid fps.
        let invalid_fps = ferryx_rd_proto::StreamConfigurationRequest {
            request_id: 102,
            desired: ferryx_rd_proto::StreamConfiguration {
                width: 1920,
                height: 1080,
                bitrate: 5_000_000,
                frames_per_second: 0,
            },
        };
        send_tcp_control(&mut tcp, ControlMessage::StreamConfigRequest(invalid_fps)).unwrap();
        let reject_frame = tcp.read_frame().unwrap();
        let (_, payload) = decode_tcp_packet(&reject_frame);
        match ControlMessage::decode(payload).unwrap() {
            ControlMessage::StreamConfigReject(rej) => {
                assert_eq!(rej.request_id, 102);
                assert_eq!(rej.reason, StreamConfigurationErrorCode::UnsupportedFps);
            }
            other => panic!("expected reject, got {other:?}"),
        }

        // When: client requests valid configuration matching host display.
        let valid_req = ferryx_rd_proto::StreamConfigurationRequest {
            request_id: 103,
            desired: ferryx_rd_proto::StreamConfiguration {
                width: 640,
                height: 360,
                bitrate: 8_000_000,
                frames_per_second: 60,
            },
        };
        send_tcp_control(&mut tcp, ControlMessage::StreamConfigRequest(valid_req)).unwrap();
        let resp_frame = tcp.read_frame().unwrap();
        let (_, payload) = decode_tcp_packet(&resp_frame);
        match ControlMessage::decode(payload).unwrap() {
            ControlMessage::StreamConfigResponse(resp) => {
                assert_eq!(resp.request_id, 103);
                assert_eq!(resp.active, valid_req.desired);
            }
            other => panic!("expected response, got {other:?}"),
        }

        // When: client requests different resolution, host applies bitrate but returns actual active geometry.
        let diff_dim = ferryx_rd_proto::StreamConfigurationRequest {
            request_id: 104,
            desired: ferryx_rd_proto::StreamConfiguration {
                width: 1920,
                height: 1080,
                bitrate: 10_000_000,
                frames_per_second: 60,
            },
        };
        send_tcp_control(&mut tcp, ControlMessage::StreamConfigRequest(diff_dim)).unwrap();
        let resp_frame = tcp.read_frame().unwrap();
        let (_, payload) = decode_tcp_packet(&resp_frame);
        match ControlMessage::decode(payload).unwrap() {
            ControlMessage::StreamConfigResponse(resp) => {
                assert_eq!(resp.request_id, 104);
                assert_eq!(resp.active.width, 640);
                assert_eq!(resp.active.height, 360);
                assert_eq!(resp.active.bitrate, 10_000_000);
            }
            other => panic!("expected response, got {other:?}"),
        }

        // Then: host updated encoder with negotiated bitrates.
        assert_eq!(*bitrates.lock().unwrap(), vec![8_000_000, 10_000_000]);

        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        server_thread.join().unwrap();
    }

    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TrackingMediaSource {
        started_count: Arc<AtomicUsize>,
    }

    struct TrackingMediaHandle;
    impl MediaHandle for TrackingMediaHandle {
        fn force_key_frame(&self) -> Result<(), SessionError> {
            Ok(())
        }
        fn update_bitrate(&self, _bitrate: u32) -> Result<(), SessionError> {
            Ok(())
        }
        fn stop(&mut self) {}
    }

    impl MediaSource for TrackingMediaSource {
        fn start_with_capabilities(
            &self,
            _sender: SyncSender<MediaEvent>,
            _peer: Capabilities,
        ) -> Result<Box<dyn MediaHandle>, SessionError> {
            self.started_count.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(TrackingMediaHandle))
        }
    }

    fn make_handshake_packet(pairing_id: &str, session_salt: [u8; 16]) -> Vec<u8> {
        let handshake = Handshake {
            name: "test-client".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: pairing_id.to_string(),
            session_salt,
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        packet
    }

    #[test]
    fn host_grants_preserve_legacy_owner_and_expiring_permissions() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        assert_eq!(
            store.authorization("legacy").unwrap(),
            (PairingTrust::Owner, None)
        );
        let permissions = GuestPermissions {
            keyboard: false,
            mouse: true,
            gamepad: false,
            clipboard: false,
            audio: false,
        };
        let trust = PairingTrust::GuestExpiring {
            expires_at_unix_ms: 123,
        };
        store
            .set_authorization("guest", trust, Some(permissions))
            .unwrap();
        let reloaded = PairingStore::new(directory.path().join("keys.json"));
        assert_eq!(
            reloaded.authorization("guest").unwrap(),
            (trust, Some(permissions))
        );
    }

    #[test]
    fn revoke_removes_pairing_grant() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        store
            .save(PairingRecord {
                id: "guest".into(),
                name: "guest".into(),
                key: [7; 32],
                added_at_unix_ms: 0,
            })
            .unwrap();
        let trust = PairingTrust::GuestExpiring {
            expires_at_unix_ms: u64::MAX,
        };
        store.set_authorization("guest", trust, None).unwrap();
        store
            .set_authorization("other", PairingTrust::Owner, None)
            .unwrap();
        assert_eq!(store.authorization("guest").unwrap(), (trust, None));

        assert!(store.revoke("guest").unwrap());
        let grants = store.read_grants().unwrap();
        assert!(!grants.contains_key("guest"));
        assert!(grants.contains_key("other"));
        assert!(store.load("guest").unwrap().is_none());

        // A grant without a key record is still cleaned up.
        assert!(!store.revoke("other").unwrap());
        assert!(store.read_grants().unwrap().is_empty());
    }

    #[test]
    fn expired_pairing_is_filtered_before_tls_acceptance() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        store
            .save(PairingRecord {
                id: "expired".into(),
                name: "expired guest".into(),
                key: [9; 32],
                added_at_unix_ms: 0,
            })
            .unwrap();
        store
            .set_authorization(
                "expired",
                PairingTrust::GuestExpiring {
                    expires_at_unix_ms: 1,
                },
                None,
            )
            .unwrap();
        let (consent, _) = mpsc::channel();
        let server = HostServer::bind_synthetic(test_config(store, consent), 0).unwrap();
        assert!(!server
            .current_psks()
            .unwrap()
            .iter()
            .any(|identity| identity.identity() == format!("{PAIRING_IDENTITY_PREFIX}expired")));
    }

    #[test]
    fn bootstrap_guest_once_ignores_owner_request_and_never_persists() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        let (consent, prompts) = mpsc::channel();
        let server = HostServer::bind_with_media(
            test_config(store.clone(), consent),
            Arc::new(TrackingMediaSource {
                started_count: Arc::new(AtomicUsize::new(0)),
            }),
        )
        .unwrap();
        let addr = server.tcp_addr().unwrap();
        let worker = thread::spawn(move || server.serve_n(1));
        let consent_worker = thread::spawn(move || {
            prompts
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .approve_as(PairingTrust::GuestOnce)
        });
        let mut tcp = TlsPskClient::new(PskIdentity::bootstrap("12345678").unwrap())
            .unwrap()
            .connect(addr)
            .unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        send_tcp_packet(
            &mut tcp,
            PacketType::PairingRequest,
            &PairingRequest {
                name: "one time".into(),
            }
            .encode()
            .unwrap(),
        )
        .unwrap();
        let packet = tcp.read_frame().unwrap();
        let grant = PairingGrant::decode(split_packet(&packet).unwrap().1).unwrap();
        assert!(store.load_all().unwrap().is_empty());
        let handshake = Handshake {
            name: "one time".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::PAIRING_TRUST
                | Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: grant.pairing_id,
            session_salt: [7; 16],
        };
        send_tcp_packet(
            &mut tcp,
            PacketType::Handshake,
            &handshake.encode().unwrap(),
        )
        .unwrap();
        assert_eq!(
            split_packet(&tcp.read_frame().unwrap())
                .unwrap()
                .0
                .packet_type,
            PacketType::HandshakeAck
        );
        send_tcp_control(
            &mut tcp,
            ControlMessage::PairingTrustRequest(ferryx_rd_proto::PairingTrustRequest {
                trust: PairingTrust::Owner,
            }),
        )
        .unwrap();
        let packet = tcp.read_frame().unwrap();
        assert!(
            matches!(ControlMessage::decode(split_packet(&packet).unwrap().1).unwrap(), ControlMessage::PairingTrustGrant(g) if g.trust == PairingTrust::GuestOnce)
        );
        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        worker.join().unwrap().unwrap();
        consent_worker.join().unwrap();
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn negotiated_guest_controls_roundtrip_and_targeted_kick() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("keys.json"));
        let key = [42; 32];
        store
            .save(PairingRecord {
                id: "owner".into(),
                name: "Owner name".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent, _) = mpsc::channel();
        let server = HostServer::bind_with_media(
            test_config(store.clone(), consent),
            Arc::new(TrackingMediaSource {
                started_count: Arc::new(AtomicUsize::new(0)),
            }),
        )
        .unwrap();
        let registry = server.guest_registry();
        let events = registry.subscribe();
        let addr = server.tcp_addr().unwrap();
        let worker = thread::spawn(move || server.serve_n(1));
        let mut tcp = TlsPskClient::new(PskIdentity::pairing("owner", &key).unwrap())
            .unwrap()
            .connect(addr)
            .unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let handshake = Handshake {
            name: "not authoritative".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: host_guest_capabilities() | Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: "owner".into(),
            session_salt: [7; 16],
        };
        send_tcp_packet(
            &mut tcp,
            PacketType::Handshake,
            &handshake.encode().unwrap(),
        )
        .unwrap();
        assert_eq!(
            split_packet(&tcp.read_frame().unwrap())
                .unwrap()
                .0
                .packet_type,
            PacketType::HandshakeAck
        );
        let read_control = |tcp: &mut ferryx_rd_net::TlsPskStream<TcpStream>| {
            let packet = tcp.read_frame().unwrap();
            ControlMessage::decode(split_packet(&packet).unwrap().1).unwrap()
        };
        assert!(
            matches!(read_control(&mut tcp), ControlMessage::PairingTrustGrant(g) if g.trust == PairingTrust::Owner)
        );
        assert!(matches!(
            read_control(&mut tcp),
            ControlMessage::GuestPermissions(_)
        ));
        assert!(
            matches!(read_control(&mut tcp), ControlMessage::HostStatus(s) if s.guest_names == vec!["Owner name"])
        );
        let guest = match events.recv_timeout(Duration::from_secs(3)).unwrap() {
            crate::guests::HostEvent::GuestConnected(g) => g,
            event => panic!("unexpected event {event:?}"),
        };
        send_tcp_control(
            &mut tcp,
            ControlMessage::RttPing(ferryx_rd_proto::RttProbe {
                timestamp_us: 123456,
            }),
        )
        .unwrap();
        assert!(
            matches!(read_control(&mut tcp), ControlMessage::RttPong(p) if p.timestamp_us == 123456)
        );
        let permissions = GuestPermissions {
            keyboard: false,
            mouse: true,
            gamepad: false,
            clipboard: false,
            audio: false,
        };
        registry
            .set_guest_permissions("owner", permissions)
            .unwrap();
        assert!(
            matches!(read_control(&mut tcp), ControlMessage::GuestPermissions(p) if p == permissions)
        );
        assert_eq!(store.authorization("owner").unwrap().1, Some(permissions));
        assert_eq!(
            host_guest_capabilities().contains(Capabilities::SECURE_ATTENTION),
            cfg!(target_os = "windows")
        );
        #[cfg(target_os = "windows")]
        {
            send_tcp_control(
                &mut tcp,
                ControlMessage::SecureAttentionRequest(ferryx_rd_proto::SecureAttentionRequest {
                    request_id: 17,
                }),
            )
            .unwrap();
            assert!(
                matches!(read_control(&mut tcp), ControlMessage::SecureAttentionResponse(r) if r.request_id == 17 && r.result == ferryx_rd_proto::SecureAttentionResult::Unsupported)
            );
        }
        assert!(registry.kick(&guest.id));
        assert!(
            matches!(read_control(&mut tcp), ControlMessage::SessionEnded(e) if e.reason == SessionEndReason::Kicked)
        );
        worker.join().unwrap().unwrap();
        assert!(registry.sessions().is_empty());
        assert!(store.load("owner").unwrap().is_some());
    }

    #[test]
    fn test_bootstrap_without_consent_handshake_rejected_with_no_capture_or_input() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key_a = [0x42; 32];
        store
            .save(PairingRecord {
                id: "PAIR_A".into(),
                name: "client-A".into(),
                key: key_a,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _consent_rx) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let media_starts = Arc::new(AtomicUsize::new(0));
        let server = HostServer::bind_with_media(
            config,
            Arc::new(TrackingMediaSource {
                started_count: Arc::clone(&media_starts),
            }),
        )
        .unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1));

        let client = TlsPskClient::new(PskIdentity::bootstrap("12345678").unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        // Transmit unauthenticated Control and InputEvent packets before handshake; host must refuse them
        let control_packet = {
            let mut p = PacketHeader::new(PacketType::Control, 0, 0, 0)
                .encode()
                .unwrap();
            p.extend_from_slice(&ControlMessage::Ping.encode().unwrap());
            p
        };
        tcp.write_frame(&control_packet).unwrap();

        let input_packet = {
            let event = InputEvent {
                event_type: ferryx_rd_proto::InputEventType::MouseMove,
                x: 100.0,
                y: 100.0,
                key_code: 0,
                modifiers: ferryx_rd_proto::Modifiers::empty(),
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            };
            let mut p = PacketHeader::new(PacketType::InputEvent, 0, 0, 0)
                .encode()
                .unwrap();
            p.extend_from_slice(&event.encode().unwrap());
            p
        };
        tcp.write_frame(&input_packet).unwrap();

        let handshake_packet = make_handshake_packet("PAIR_A", [0x11; 16]);
        tcp.write_frame(&handshake_packet).unwrap();

        let read_result = tcp.read_frame();
        let _ = send_tcp_control(&mut tcp, ControlMessage::Disconnect);
        drop(tcp);
        let server_result = server_thread.join().unwrap();

        assert!(
            matches!(server_result, Err(SessionError::PreAuth)),
            "server must reject unconsented bootstrap handshake with PreAuth, got: {server_result:?}"
        );
        assert_eq!(
            media_starts.load(Ordering::SeqCst),
            0,
            "media capture must not start without consent"
        );
        if let Ok(frame) = read_result {
            let (hdr, _) = decode_tcp_packet(&frame);
            assert_ne!(
                hdr.packet_type,
                PacketType::HandshakeAck,
                "server must not send HandshakeAck for unconsented handshake"
            );
            assert_ne!(
                hdr.packet_type,
                PacketType::Control,
                "server must not send Control/InputAck for unauthenticated input"
            );
        }
    }

    #[test]
    fn test_bootstrap_consent_b_cannot_use_a() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key_a = [0x42; 32];
        store
            .save(PairingRecord {
                id: "PAIR_A".into(),
                name: "client-A".into(),
                key: key_a,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, consent_rx) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let media_starts = Arc::new(AtomicUsize::new(0));
        let server = HostServer::bind_with_media(
            config,
            Arc::new(TrackingMediaSource {
                started_count: Arc::clone(&media_starts),
            }),
        )
        .unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1));
        let consent_thread = thread::spawn(move || {
            let prompt: ConsentPrompt = consent_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(prompt.client_name, "client-B");
            prompt.approve();
        });

        let client = TlsPskClient::new(PskIdentity::bootstrap("12345678").unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let request = PairingRequest {
            name: "client-B".into(),
        };
        let mut req_packet = PacketHeader::new(PacketType::PairingRequest, 0, 0, 0)
            .encode()
            .unwrap();
        req_packet.extend_from_slice(&request.encode().unwrap());
        tcp.write_frame(&req_packet).unwrap();

        let grant_frame = tcp.read_frame().unwrap();
        let (grant_hdr, grant_payload) = decode_tcp_packet(&grant_frame);
        assert_eq!(grant_hdr.packet_type, PacketType::PairingGrant);
        let _grant = PairingGrant::decode(grant_payload).unwrap();
        consent_thread.join().unwrap();

        let handshake_packet = make_handshake_packet("PAIR_A", [0x22; 16]);
        tcp.write_frame(&handshake_packet).unwrap();

        let read_result = tcp.read_frame();
        let _ = send_tcp_control(&mut tcp, ControlMessage::Disconnect);
        drop(tcp);
        let server_result = server_thread.join().unwrap();

        assert!(
            matches!(server_result, Err(SessionError::IdentityMismatch)),
            "server must reject handshake with mismatched pairing ID with IdentityMismatch, got: {server_result:?}"
        );
        assert_eq!(
            media_starts.load(Ordering::SeqCst),
            0,
            "media capture must not start on mismatched ID"
        );
        if let Ok(frame) = read_result {
            let (hdr, _) = decode_tcp_packet(&frame);
            assert_ne!(
                hdr.packet_type,
                PacketType::HandshakeAck,
                "server must not send HandshakeAck for mismatched pairing ID"
            );
        }
    }

    #[test]
    fn test_bootstrap_normal_b_and_paired_a_work() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key_a = [0x42; 32];
        store
            .save(PairingRecord {
                id: "PAIR_A".into(),
                name: "client-A".into(),
                key: key_a,
                added_at_unix_ms: 0,
            })
            .unwrap();

        // Part 1: Normal B works over bootstrap TLS
        {
            let (consent_tx, consent_rx) = mpsc::channel();
            let config = test_config(store.clone(), consent_tx);
            let media_starts = Arc::new(AtomicUsize::new(0));
            let server = HostServer::bind_with_media(
                config,
                Arc::new(TrackingMediaSource {
                    started_count: Arc::clone(&media_starts),
                }),
            )
            .unwrap();
            let tcp_addr = server.tcp_addr().unwrap();
            let server_thread = thread::spawn(move || server.serve_n(1));
            let consent_thread = thread::spawn(move || {
                let prompt: ConsentPrompt =
                    consent_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                assert_eq!(prompt.client_name, "client-B");
                prompt.approve();
            });

            let client = TlsPskClient::new(PskIdentity::bootstrap("12345678").unwrap()).unwrap();
            let mut tcp = client.connect(tcp_addr).unwrap();
            tcp.ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();

            let request = PairingRequest {
                name: "client-B".into(),
            };
            let mut req_packet = PacketHeader::new(PacketType::PairingRequest, 0, 0, 0)
                .encode()
                .unwrap();
            req_packet.extend_from_slice(&request.encode().unwrap());
            tcp.write_frame(&req_packet).unwrap();

            let grant_frame = tcp.read_frame().unwrap();
            let (grant_hdr, grant_payload) = decode_tcp_packet(&grant_frame);
            assert_eq!(grant_hdr.packet_type, PacketType::PairingGrant);
            let grant = PairingGrant::decode(grant_payload).unwrap();
            consent_thread.join().unwrap();

            let handshake_packet = make_handshake_packet(&grant.pairing_id, [0x33; 16]);
            tcp.write_frame(&handshake_packet).unwrap();

            let ack_frame = tcp.read_frame().unwrap();
            let (ack_hdr, _) = decode_tcp_packet(&ack_frame);
            assert_eq!(ack_hdr.packet_type, PacketType::HandshakeAck);
            assert_eq!(media_starts.load(Ordering::SeqCst), 1);

            send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
            assert!(server_thread.join().unwrap().is_ok());
        }

        // Part 2: Paired A works over pairing TLS
        {
            let (consent_tx, _consent_rx) = mpsc::channel();
            let config = test_config(store.clone(), consent_tx);
            let media_starts = Arc::new(AtomicUsize::new(0));
            let server = HostServer::bind_with_media(
                config,
                Arc::new(TrackingMediaSource {
                    started_count: Arc::clone(&media_starts),
                }),
            )
            .unwrap();
            let tcp_addr = server.tcp_addr().unwrap();
            let server_thread = thread::spawn(move || server.serve_n(1));

            let client =
                TlsPskClient::new(PskIdentity::pairing("PAIR_A", &key_a).unwrap()).unwrap();
            let mut tcp = client.connect(tcp_addr).unwrap();
            tcp.ssl_stream()
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();

            let handshake_packet = make_handshake_packet("PAIR_A", [0x44; 16]);
            tcp.write_frame(&handshake_packet).unwrap();

            let ack_frame = tcp.read_frame().unwrap();
            let (ack_hdr, _) = decode_tcp_packet(&ack_frame);
            assert_eq!(ack_hdr.packet_type, PacketType::HandshakeAck);
            assert_eq!(media_starts.load(Ordering::SeqCst), 1);

            send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
            assert!(server_thread.join().unwrap().is_ok());
        }
    }

    #[test]
    fn test_bootstrap_authenticated_session_rejects_duplicate_handshake() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key_a = [0x42; 32];
        store
            .save(PairingRecord {
                id: "PAIR_A".into(),
                name: "client-A".into(),
                key: key_a,
                added_at_unix_ms: 0,
            })
            .unwrap();

        let (consent_tx, _consent_rx) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let media_starts = Arc::new(AtomicUsize::new(0));
        let server = HostServer::bind_with_media(
            config,
            Arc::new(TrackingMediaSource {
                started_count: Arc::clone(&media_starts),
            }),
        )
        .unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1));

        let client = TlsPskClient::new(PskIdentity::pairing("PAIR_A", &key_a).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        // 1. First Handshake: must succeed
        let handshake_packet = make_handshake_packet("PAIR_A", [0x55; 16]);
        tcp.write_frame(&handshake_packet).unwrap();
        let ack_frame = tcp.read_frame().unwrap();
        assert_eq!(
            decode_tcp_packet(&ack_frame).0.packet_type,
            PacketType::HandshakeAck
        );
        assert_eq!(media_starts.load(Ordering::SeqCst), 1);

        // 2. Second Handshake on authenticated session: must be rejected with error and closed
        let duplicate_handshake = make_handshake_packet("PAIR_A", [0x66; 16]);
        tcp.write_frame(&duplicate_handshake).unwrap();

        let read_second = tcp.read_frame();
        let _ = send_tcp_control(&mut tcp, ControlMessage::Disconnect);
        drop(tcp);
        let server_result = server_thread.join().unwrap();

        assert!(
            matches!(server_result, Err(SessionError::AlreadyAuthenticated)),
            "server must reject duplicate handshake with AlreadyAuthenticated, got: {server_result:?}"
        );
        assert_eq!(
            media_starts.load(Ordering::SeqCst),
            1,
            "media capture must not restart on duplicate handshake"
        );
        if let Ok(frame) = read_second {
            let (hdr, _) = decode_tcp_packet(&frame);
            assert_ne!(
                hdr.packet_type,
                PacketType::HandshakeAck,
                "server must not send HandshakeAck for duplicate handshake"
            );
        }
    }

    #[test]
    fn test_serve_with_stop_exits_when_flag_set() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let (consent_tx, _consent_rx) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let server = HostServer::bind(config).unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let server_thread = thread::spawn(move || server.serve_with_stop(stop_clone));

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let result = server_thread.join().expect("server thread joined");
        assert!(result.is_ok());
    }

    #[test]
    fn test_udp_discover_peer_rejects_invalid_packets_and_fixes_endpoint() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let (consent_tx, _consent_rx) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let server = HostServer::bind(config).unwrap();
        let host_udp_addr = server.udp_socket.local_addr().unwrap();

        let key = [0x77; 32];
        let salt = [0x88; 16];
        let prior_salt = [0x33; 16];

        // Independent client-send and host-receive cipher instances
        let mut client_c2h = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
        let mut host_c2h = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
        let mut host_h2c = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let mut client_h2c = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let mut prior_client_c2h =
            DatagramCipher::derive(&key, &prior_salt, Direction::ClientToHost).unwrap();

        let client_udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_udp
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let client_addr = client_udp.local_addr().unwrap();

        let attacker_udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        attacker_udp.set_nonblocking(true).unwrap();
        let attacker_addr = attacker_udp.local_addr().unwrap();

        let tcp_peer = client_addr; // Same IP: 127.0.0.1
        let mut udp_peer: Option<SocketAddr> = None;

        let mut peek_buf = [0_u8; 2048];
        let mut check_buf = [0_u8; 64];

        // Stage 1a: Plaintext 0xff probe from attacker must be consumed but NOT set udp_peer
        attacker_udp.send_to(&[0xff], host_udp_addr).unwrap();
        let (peek_len, peek_from) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, 1);
        assert_eq!(peek_from, attacker_addr);
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(udp_peer, None, "plaintext probe must not register endpoint");
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "plaintext probe must have been consumed from socket"
        );

        // Stage 1b: Short malformed packet (< 40 bytes)
        let malformed = [0x45, 0x52, 0x07, 0x00, 0x01, 0x02];
        attacker_udp.send_to(&malformed, host_udp_addr).unwrap();
        let (peek_len, _) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, malformed.len());
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(udp_peer, None, "short packet must not register endpoint");
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "short packet must have been consumed from socket"
        );

        // Stage 1c: Packet encrypted with wrong key
        let wrong_key = [0x99; 32];
        let mut wrong_cipher =
            DatagramCipher::derive(&wrong_key, &salt, Direction::ClientToHost).unwrap();
        let ping_hdr = PacketHeader::new(PacketType::Ping, 0, 1000, 0);
        let wrong_packet = wrong_cipher.seal_datagram(&ping_hdr, &[]).unwrap();
        attacker_udp.send_to(&wrong_packet, host_udp_addr).unwrap();
        let (peek_len, _) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, wrong_packet.len());
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(
            udp_peer, None,
            "wrong-key packet must not register endpoint"
        );
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "wrong-key packet must have been consumed from socket"
        );

        // Stage 1d: Non-registration packet (valid encryption, but PacketType::InputEvent)
        let input_hdr = PacketHeader::new(PacketType::InputEvent, 1, 1000, 0);
        let non_reg_packet = client_c2h.seal_datagram(&input_hdr, &[]).unwrap();
        client_udp.send_to(&non_reg_packet, host_udp_addr).unwrap();
        let (peek_len, _) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, non_reg_packet.len());
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(
            udp_peer, None,
            "non-registration packet must not register endpoint"
        );
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "non-registration packet must have been consumed from socket"
        );

        // Stage 1e: Ping with non-empty payload
        let ping_payload_hdr = PacketHeader::new(PacketType::Ping, 2, 1000, 0);
        let payload_packet = client_c2h
            .seal_datagram(&ping_payload_hdr, &[1, 2, 3])
            .unwrap();
        client_udp.send_to(&payload_packet, host_udp_addr).unwrap();
        let (peek_len, _) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, payload_packet.len());
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(
            udp_peer, None,
            "ping with non-empty payload must not register endpoint"
        );
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "non-empty ping must have been consumed from socket"
        );

        // Stage 1f: Prior-session salt packet
        let prior_hdr = PacketHeader::new(PacketType::Ping, 0, 1000, 0);
        let prior_packet = prior_client_c2h.seal_datagram(&prior_hdr, &[]).unwrap();
        client_udp.send_to(&prior_packet, host_udp_addr).unwrap();
        let (peek_len, _) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, prior_packet.len());
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(
            udp_peer, None,
            "prior-session packet must not register endpoint"
        );
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "prior-session packet must have been consumed from socket"
        );

        // Stage 2: Genuine registration packet from legitimate client (sealed empty Ping)
        let reg_hdr = PacketHeader::new(PacketType::Ping, 3, 1000, 0);
        let reg_packet = client_c2h.seal_datagram(&reg_hdr, &[]).unwrap();
        client_udp.send_to(&reg_packet, host_udp_addr).unwrap();
        let (peek_len, _) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, reg_packet.len());
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(
            udp_peer,
            Some(client_addr),
            "valid registration must register legitimate client endpoint"
        );

        // Stage 3: Controlled frame delivery to registered valid socket
        let frame_hdr = PacketHeader::new(PacketType::FrameHeader, 1, 1000, 0);
        let frame_payload = b"controlled-test-frame-content";
        let sealed_frame = host_h2c.seal_datagram(&frame_hdr, frame_payload).unwrap();
        server
            .udp_socket
            .send_to(&sealed_frame, udp_peer.unwrap())
            .unwrap();

        let mut recv_buf = [0_u8; 1024];
        let (recv_len, from_addr) = client_udp.recv_from(&mut recv_buf).unwrap();
        assert_eq!(from_addr, host_udp_addr);
        let (opened_hdr, opened_payload) = client_h2c.open_datagram(&recv_buf[..recv_len]).unwrap();
        assert_eq!(opened_hdr.packet_type, PacketType::FrameHeader);
        assert_eq!(opened_payload, frame_payload);

        // Attacker received nothing
        let mut attacker_buf = [0_u8; 1024];
        assert!(
            matches!(attacker_udp.recv_from(&mut attacker_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock)
        );

        // Stage 4: Post-registration hijack attempt from attacker socket
        let hijack_hdr = PacketHeader::new(PacketType::Ping, 4, 1000, 0);
        let hijack_packet = client_c2h.seal_datagram(&hijack_hdr, &[]).unwrap();
        attacker_udp.send_to(&hijack_packet, host_udp_addr).unwrap();
        let (peek_len, _) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, hijack_packet.len());
        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(
            udp_peer,
            Some(client_addr),
            "post-registration packet must NOT change registered endpoint"
        );
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "hijack packet must have been consumed from socket"
        );

        // Another frame sent still reaches legitimate client, not attacker
        let frame_hdr2 = PacketHeader::new(PacketType::FrameHeader, 2, 2000, 0);
        let sealed_frame2 = host_h2c.seal_datagram(&frame_hdr2, frame_payload).unwrap();
        server
            .udp_socket
            .send_to(&sealed_frame2, udp_peer.unwrap())
            .unwrap();

        let (recv_len2, _) = client_udp.recv_from(&mut recv_buf).unwrap();
        let (opened_hdr2, _) = client_h2c.open_datagram(&recv_buf[..recv_len2]).unwrap();
        assert_eq!(opened_hdr2.sequence, 2);
        assert!(
            matches!(attacker_udp.recv_from(&mut attacker_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock)
        );
    }

    #[test]
    fn test_udp_registration_rejects_prior_session_datagram() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let (consent_tx, _consent_rx) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let server = HostServer::bind(config).unwrap();
        let host_udp_addr = server.udp_socket.local_addr().unwrap();

        let key = [0x77; 32];
        let current_salt = [0x88; 16];
        let prior_salt = [0x11; 16];

        // Independent client-send with prior salt and host-receive with current salt
        let mut prior_client_c2h =
            DatagramCipher::derive(&key, &prior_salt, Direction::ClientToHost).unwrap();
        let mut host_c2h =
            DatagramCipher::derive(&key, &current_salt, Direction::ClientToHost).unwrap();

        let client_udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_udp.set_nonblocking(true).unwrap();
        let client_addr = client_udp.local_addr().unwrap();

        let tcp_peer = client_addr;
        let mut udp_peer: Option<SocketAddr> = None;

        // Packet sealed with prior session's salt
        let reg_hdr = PacketHeader::new(PacketType::Ping, 0, 1000, 0);
        let prior_packet = prior_client_c2h.seal_datagram(&reg_hdr, &[]).unwrap();
        client_udp.send_to(&prior_packet, host_udp_addr).unwrap();

        // Prove packet arrived in host socket queue
        let mut peek_buf = [0_u8; 2048];
        let (peek_len, peek_from) = peek_datagram(&server.udp_socket, &mut peek_buf);
        assert_eq!(peek_len, prior_packet.len());
        assert_eq!(peek_from, client_addr);

        server
            .discover_udp_peer(tcp_peer, &mut udp_peer, Some(&mut host_c2h))
            .unwrap();
        assert_eq!(
            udp_peer, None,
            "datagram from prior session must not register endpoint"
        );

        // Prove packet was consumed and discarded by discover_udp_peer
        let mut check_buf = [0_u8; 64];
        assert!(
            matches!(server.udp_socket.recv_from(&mut check_buf), Err(ref e) if e.kind() == io::ErrorKind::WouldBlock),
            "prior-session packet must have been consumed from socket"
        );
    }

    #[test]
    fn test_udp_host_rejects_client_missing_authenticated_registration_capability() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x33; 32];
        store
            .save(PairingRecord {
                id: "PAIR_TEST".into(),
                name: "client-test".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();

        let (consent_tx, _consent_rx) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let media_starts = Arc::new(AtomicUsize::new(0));
        let server = HostServer::bind_with_media(
            config,
            Arc::new(TrackingMediaSource {
                started_count: Arc::clone(&media_starts),
            }),
        )
        .unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1));

        let client = TlsPskClient::new(PskIdentity::pairing("PAIR_TEST", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        // Client sends handshake with Capabilities::empty() (missing AUTHENTICATED_UDP_REGISTRATION)
        let handshake = Handshake {
            name: "test-client".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::empty(),
            pairing_id: "PAIR_TEST".into(),
            session_salt: [0x55; 16],
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();

        // Host must terminate without sending HandshakeAck and without starting media
        let read_res = tcp.read_frame();
        if let Ok(frame) = read_res {
            let (hdr, _) = decode_tcp_packet(&frame);
            assert_ne!(
                hdr.packet_type,
                PacketType::HandshakeAck,
                "host must NOT send HandshakeAck to client lacking authenticated UDP registration capability"
            );
        }
        let server_res = server_thread.join().unwrap();
        assert!(
            server_res.is_err(),
            "server must return error when client lacks capability"
        );
        assert_eq!(
            media_starts.load(Ordering::SeqCst),
            0,
            "media capture must NOT start when client lacks capability"
        );
    }

    #[test]
    fn test_watchdog_should_abort_truth_table() {
        let timeout = Duration::from_secs(30);

        // When finished is true, never abort regardless of idle duration
        assert!(!watchdog_should_abort(true, timeout, timeout));
        assert!(!watchdog_should_abort(
            true,
            timeout + Duration::from_secs(10),
            timeout
        ));
        assert!(!watchdog_should_abort(true, Duration::ZERO, timeout));

        // When not finished:
        // idle >= timeout triggers abort
        assert!(watchdog_should_abort(
            false,
            timeout + Duration::from_millis(1),
            timeout
        ));
        assert!(watchdog_should_abort(false, timeout, timeout));

        // idle < timeout does not trigger abort
        assert!(!watchdog_should_abort(
            false,
            timeout - Duration::from_millis(1),
            timeout
        ));
        assert!(!watchdog_should_abort(false, Duration::ZERO, timeout));
    }

    #[test]
    fn join_with_deadline_reaps_a_finished_worker() {
        let handle = thread::spawn(|| {});
        assert!(join_with_deadline(handle, Duration::from_secs(5)));
    }

    #[test]
    fn join_with_deadline_gives_up_on_a_wedged_worker() {
        // Session teardown must never wait indefinitely on the UDP sender. A
        // sender that never exits used to hold the join open, and because the
        // media pipeline was stopped after that join, its capture and encode
        // workers leaked and kept spinning at full CPU.
        let release = Arc::new(AtomicBool::new(false));
        let worker_release = Arc::clone(&release);
        let handle = thread::spawn(move || {
            while !worker_release.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(5));
            }
        });

        let started = Instant::now();
        assert!(!join_with_deadline(handle, Duration::from_millis(50)));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "teardown must not block on a wedged worker"
        );

        release.store(true, Ordering::Relaxed);
    }

    #[test]
    fn stop_pipeline_stops_a_registered_pipeline_exactly_once() {
        // The watchdog's only lever on a wedged session. The owning thread never
        // unwinds, so nothing it owns is dropped and the pipeline has to be
        // stopped from outside or its workers run forever.
        struct CountingHandle(Arc<AtomicUsize>);
        impl MediaHandle for CountingHandle {
            fn force_key_frame(&self) -> Result<(), SessionError> {
                Ok(())
            }
            fn update_bitrate(&self, _bitrate: u32) -> Result<(), SessionError> {
                Ok(())
            }
            fn stop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let stops = Arc::new(AtomicUsize::new(0));
        let slot: PipelineSlot = Arc::new(Mutex::new(None));
        assert!(!stop_pipeline(&slot), "an empty slot has nothing to stop");

        *slot.lock().unwrap() = Some(Box::new(CountingHandle(Arc::clone(&stops))));
        assert!(stop_pipeline(&slot));
        assert_eq!(stops.load(Ordering::Relaxed), 1);

        // Taking the handle out is what keeps the session thread from stopping
        // it a second time as its own teardown unwinds.
        assert!(!stop_pipeline(&slot));
        assert_eq!(stops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_session_watch_lifecycle() {
        let timeout = Duration::from_secs(10);
        let watch = SessionWatch::new(timeout);

        // Fresh watch is not aborted and has timeout set
        assert!(!watch.is_aborted());
        assert_eq!(watch.timeout(), timeout);
        assert!(watch.idle(Instant::now()) < Duration::from_millis(50));

        // Simulate time advancing without a tick: idle grows
        let origin = Instant::now();
        let t1 = origin + Duration::from_secs(5);
        assert!(watch.idle(t1) >= Duration::from_secs(5));

        // tick pushes idle back down
        watch.tick(t1);
        assert!(watch.idle(t1) < Duration::from_millis(50));

        // abort flips is_aborted
        watch.abort();
        assert!(watch.is_aborted());
    }

    #[test]
    fn test_reprobe_output_if_missing() {
        let monitors_with_headless_focused: serde_json::Value = serde_json::json!([
            {"name": "HEADLESS-1", "focused": true},
            {"name": "DP-1", "focused": false}
        ]);
        let monitors_only_headless: serde_json::Value = serde_json::json!([
            {"name": "HEADLESS-1", "focused": true}
        ]);
        let monitors_healthy: serde_json::Value = serde_json::json!([
            {"name": "DP-1", "focused": false},
            {"name": "HDMI-A-2", "focused": true}
        ]);

        // 1. Pinned "HDMI-A-2" missing from a list where HEADLESS-1 is focused plus DP-1
        // -> returns DP-1, NOT HEADLESS-1
        assert_eq!(
            reprobe_output_if_missing(Some("HDMI-A-2"), Some(&monitors_with_headless_focused)),
            Some("DP-1".to_string())
        );

        // 2. Pinned missing, only HEADLESS-1 present -> returns HEADLESS-1
        assert_eq!(
            reprobe_output_if_missing(Some("HDMI-A-2"), Some(&monitors_only_headless)),
            Some("HEADLESS-1".to_string())
        );

        // 3. Pinned present -> unchanged
        assert_eq!(
            reprobe_output_if_missing(Some("HDMI-A-2"), Some(&monitors_healthy)),
            Some("HDMI-A-2".to_string())
        );

        // 4. monitors = None -> pinned unchanged
        assert_eq!(
            reprobe_output_if_missing(Some("HDMI-A-2"), None),
            Some("HDMI-A-2".to_string())
        );

        // 5. No pinned name falls back to focused monitor
        assert_eq!(
            reprobe_output_if_missing(None, Some(&monitors_healthy)),
            Some("HDMI-A-2".to_string())
        );
    }

    #[test]
    fn test_isolation_stalled_connection_does_not_block_subsequent_connection() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x5a; 32];
        store
            .save(PairingRecord {
                id: "client-b".into(),
                name: "fixture-b".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let mut server = HostServer::bind_synthetic(config, 100).unwrap();
        server.preauth_timeout = Duration::from_secs(30);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let tcp_addr = server.tcp_addr().unwrap();
        let udp_addr = server.udp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_with_stop(stop_clone));

        // Connection A: connect raw TCP stream and send nothing (parks A in preauth read for up to 30s)
        let sock_a = TcpStream::connect(tcp_addr).unwrap();

        // Connection B: connect, complete TLS, handshake, and register UDP within a few seconds
        let client_b = TlsPskClient::new(PskIdentity::pairing("client-b", &key).unwrap()).unwrap();
        let mut tcp_b = client_b.connect(tcp_addr).unwrap();
        tcp_b
            .ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();

        let salt = [0x44; 16];
        let handshake = Handshake {
            name: "client-b".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: "client-b".into(),
            session_salt: salt,
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp_b.write_frame(&packet).unwrap();
        let ack = tcp_b.read_frame().unwrap();
        assert_eq!(
            decode_tcp_packet(&ack).0.packet_type,
            PacketType::HandshakeAck
        );

        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut c2h = DatagramCipher::derive(&key, &salt, Direction::ClientToHost).unwrap();
        let reg = c2h
            .seal_datagram(&PacketHeader::new(PacketType::Ping, 0, 0, 0), &[])
            .unwrap();
        udp.send_to(&reg, udp_addr).unwrap();

        let mut receive = DatagramCipher::derive(&key, &salt, Direction::HostToClient).unwrap();
        let mut buffer = [0_u8; 2048];
        // The synthetic source interleaves video frames with its pings, so the
        // proof is any datagram that authenticates with this session's key.
        let media_deadline = Instant::now() + Duration::from_secs(3);
        let mut received = false;
        while Instant::now() < media_deadline {
            let Ok((length, _)) = udp.recv_from(&mut buffer) else {
                break;
            };
            if receive.open_datagram(&buffer[..length]).is_ok() {
                received = true;
                break;
            }
        }
        assert!(
            received,
            "a second client must receive host media while a stalled peer holds its own session"
        );

        // Teardown
        let _ = send_tcp_control(&mut tcp_b, ControlMessage::Disconnect);
        drop(tcp_b);
        drop(sock_a);
        stop.store(true, Ordering::Relaxed);
        let _ = server_thread.join();
    }

    #[test]
    fn test_watchdog_aborts_stalled_session() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x7b; 32];
        store
            .save(PairingRecord {
                id: "stalled-client".into(),
                name: "fixture-stall".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let mut server = HostServer::bind_synthetic(config, 100).unwrap();
        prepare_admission_input(&mut server);
        let tcp_addr = server.tcp_addr().unwrap();

        // The client's TLS handshake only completes once the server side accepts,
        // so both sides must run concurrently: the server drives accept and the
        // session on its own thread, never on the thread that owns the client.
        let (session_tx, session_rx) = mpsc::channel();
        let server_thread = thread::spawn(move || {
            let (server_tcp, peer) = server.tcp_listener.accept().unwrap();
            server_tcp.set_nodelay(true).unwrap();
            let tls_server = TlsPskServer::new(server.current_psks().unwrap()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let stream = tls_server
                .accept_stream_until(server_tcp, deadline)
                .unwrap();
            let watch = Arc::new(SessionWatch::new(Duration::from_secs(30)));
            let handler_watch = Arc::clone(&watch);
            let (result_tx, result_rx) = mpsc::channel();
            let handler = thread::spawn(move || {
                let result = server.handle_connection(
                    stream,
                    peer,
                    deadline,
                    None,
                    &handler_watch,
                    &PipelineSlot::default(),
                );
                let _ = result_tx.send(result);
            });
            session_tx.send((watch, result_rx, handler)).unwrap();
        });

        let client =
            TlsPskClient::new(PskIdentity::pairing("stalled-client", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        tcp.ssl_stream()
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (watch, handler_result, handler_thread) =
            session_rx.recv_timeout(Duration::from_secs(3)).unwrap();

        // Authenticate the session
        let salt = [0x99; 16];
        let handshake = Handshake {
            name: "stalled-client".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION,
            pairing_id: "stalled-client".into(),
            session_salt: salt,
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();
        let ack = tcp.read_frame().unwrap();
        assert_eq!(
            decode_tcp_packet(&ack).0.packet_type,
            PacketType::HandshakeAck
        );

        // Session is live. Now call watch.abort()
        watch.abort();

        // Assert handler returns Ok within bounded time
        let handler_res = handler_result
            .recv_timeout(Duration::from_secs(5))
            .expect("session handler must return once the watch is aborted");
        assert!(handler_res.is_ok());
        let _ = handler_thread.join();

        // And client observes socket closure within bounded poll
        tcp.ssl_stream_mut()
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let _ = server_thread.join();
        let poll_deadline = Instant::now() + Duration::from_secs(3);
        let mut closed = false;
        let mut buf = [0_u8; 128];
        use std::io::Read;
        while Instant::now() < poll_deadline {
            match tcp.ssl_stream_mut().read(&mut buf) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Err(ref e)
                    if e.kind() == io::ErrorKind::ConnectionReset
                        || e.kind() == io::ErrorKind::BrokenPipe
                        || e.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    closed = true;
                    break;
                }
                Err(ref e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => {
                    closed = true;
                    break;
                }
                Ok(_) => {
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
        assert!(closed, "client must observe socket closure after abort");
    }

    #[test]
    fn policy_and_display_list_sent_after_stream_options_handshake() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x88; 32];
        store
            .save(PairingRecord {
                id: "policy-test".into(),
                name: "client-options".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let config = test_config(store, consent_tx);
        let server = HostServer::bind_synthetic(config, 10).unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());

        let client = TlsPskClient::new(PskIdentity::pairing("policy-test", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let handshake = Handshake {
            name: "client-options".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION
                | Capabilities::STREAM_OPTIONS,
            pairing_id: "policy-test".into(),
            session_salt: [0x51; 16],
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();

        // Expect HandshakeAck
        let ack_frame = tcp.read_frame().unwrap();
        let (hdr, payload) = decode_tcp_packet(&ack_frame);
        assert_eq!(hdr.packet_type, PacketType::HandshakeAck);
        let ack = Handshake::decode(payload).unwrap();
        assert!(ack.capabilities.contains(Capabilities::STREAM_OPTIONS));

        // Expect HostPolicy
        let policy_frame = tcp.read_frame().unwrap();
        let (hdr, payload) = decode_tcp_packet(&policy_frame);
        assert_eq!(hdr.packet_type, PacketType::Control);
        let policy_msg = ControlMessage::decode(payload).unwrap();
        assert_eq!(
            policy_msg,
            ControlMessage::HostPolicy(HostPolicy {
                allow_input: true,
                allow_clipboard: false,
            })
        );

        // Expect HostDisplayList
        let displays_frame = tcp.read_frame().unwrap();
        let (hdr, payload) = decode_tcp_packet(&displays_frame);
        assert_eq!(hdr.packet_type, PacketType::Control);
        let displays_msg = ControlMessage::decode(payload).unwrap();
        match displays_msg {
            ControlMessage::HostDisplayList(list) => {
                assert!(!list.displays.is_empty());
                assert_eq!(list.displays[0].index, 0);
                assert!(list.displays[0].primary);
            }
            other => panic!("expected HostDisplayList, got {other:?}"),
        }

        let initial = tcp.read_frame().unwrap();
        assert!(
            matches!(ControlMessage::decode(decode_tcp_packet(&initial).1).unwrap(), ControlMessage::StreamOptionsActive(active) if active.codec == StreamCodecPreference::H264)
        );
        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        server_thread.join().unwrap();
    }

    #[test]
    fn input_with_allow_input_false_is_not_injected_and_acked_error_code_2() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x99; 32];
        store
            .save(PairingRecord {
                id: "input-gate-test".into(),
                name: "client-gate".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let config = test_config(store, consent_tx);
        config.allow_input.store(false, Ordering::SeqCst);
        let server = HostServer::bind_synthetic(config, 10).unwrap();
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());

        let client =
            TlsPskClient::new(PskIdentity::pairing("input-gate-test", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let handshake = Handshake {
            name: "client-gate".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION
                | Capabilities::STREAM_OPTIONS,
            pairing_id: "input-gate-test".into(),
            session_salt: [0x52; 16],
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();

        // Read HandshakeAck, HostPolicy, HostDisplayList
        let _ack = tcp.read_frame().unwrap();
        let policy_frame = tcp.read_frame().unwrap();
        let (_, payload) = decode_tcp_packet(&policy_frame);
        assert_eq!(
            ControlMessage::decode(payload).unwrap(),
            ControlMessage::HostPolicy(HostPolicy {
                allow_input: false,
                allow_clipboard: false,
            })
        );
        let _displays = tcp.read_frame().unwrap();
        let initial = tcp.read_frame().unwrap();
        assert!(
            matches!(ControlMessage::decode(decode_tcp_packet(&initial).1).unwrap(), ControlMessage::StreamOptionsActive(active) if active.codec == StreamCodecPreference::H264)
        );

        // Send InputEvent
        let input_evt = InputEvent {
            event_type: ferryx_rd_proto::InputEventType::MouseMove,
            x: 0.5,
            y: 0.5,
            key_code: 0,
            modifiers: ferryx_rd_proto::Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        let mut input_packet = PacketHeader::new(PacketType::InputEvent, 42, 0, 0)
            .encode()
            .unwrap();
        input_packet.extend_from_slice(&input_evt.encode().unwrap());
        tcp.write_frame(&input_packet).unwrap();

        // Expect InputAck with sequence 42, success false, error_code 2
        let ack_frame = tcp.read_frame().unwrap();
        let (hdr, payload) = decode_tcp_packet(&ack_frame);
        assert_eq!(hdr.packet_type, PacketType::Control);
        assert_eq!(
            ControlMessage::decode(payload).unwrap(),
            ControlMessage::InputAck(InputAckMessage {
                sequence: 42,
                success: false,
                error_code: 2,
            })
        );

        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        server_thread.join().unwrap();
    }

    #[test]
    fn initial_bitrate_scales_with_pixels_rate_and_codec_but_respects_cap() {
        assert_eq!(
            starting_bitrate(3840, 2160, 60, StreamCodecPreference::Hevc, 100_000_000),
            49_766_400
        );
        assert_eq!(
            starting_bitrate(1920, 1080, 60, StreamCodecPreference::H264, 100_000_000),
            20_736_000
        );
        assert_eq!(
            starting_bitrate(3840, 2160, 60, StreamCodecPreference::Hevc, 8_000_000),
            8_000_000
        );
        assert_eq!(
            starting_bitrate(
                u32::MAX,
                u32::MAX,
                240,
                StreamCodecPreference::Hevc,
                100_000_000
            ),
            100_000_000
        );
    }

    #[test]
    fn validate_stream_options_accepts_valid_request() {
        let displays = vec![HostDisplay {
            index: 0,
            width: 1920,
            height: 1080,
            primary: true,
            name: "Display 1".to_string(),
        }];
        let request = StreamOptionsRequest {
            request_id: 1,
            scale_permille: 1000,
            fps: 60,
            codec: StreamCodecPreference::Auto,
            display_index: 0,
            bitrate: 10_000_000,
        };
        assert!(validate_stream_options(&request, &displays).is_ok());
    }

    #[test]
    fn validate_stream_options_rejects_fps_zero() {
        let displays = vec![HostDisplay {
            index: 0,
            width: 1920,
            height: 1080,
            primary: true,
            name: "Display 1".to_string(),
        }];
        let request = StreamOptionsRequest {
            request_id: 1,
            scale_permille: 1000,
            fps: 0,
            codec: StreamCodecPreference::Auto,
            display_index: 0,
            bitrate: 10_000_000,
        };
        let err = validate_stream_options(&request, &displays).unwrap_err();
        assert_eq!(err.0, StreamConfigurationErrorCode::UnsupportedFps);
        assert!(!err.1.is_empty());
    }

    #[test]
    fn validate_stream_options_rejects_scale_zero() {
        let displays = vec![HostDisplay {
            index: 0,
            width: 1920,
            height: 1080,
            primary: true,
            name: "Display 1".to_string(),
        }];
        let request = StreamOptionsRequest {
            request_id: 1,
            scale_permille: 0,
            fps: 60,
            codec: StreamCodecPreference::Auto,
            display_index: 0,
            bitrate: 10_000_000,
        };
        let err = validate_stream_options(&request, &displays).unwrap_err();
        assert_eq!(err.0, StreamConfigurationErrorCode::UnsupportedDimensions);
        assert!(!err.1.is_empty());
    }

    #[test]
    fn validate_stream_options_rejects_scale_1001() {
        let displays = vec![HostDisplay {
            index: 0,
            width: 1920,
            height: 1080,
            primary: true,
            name: "Display 1".to_string(),
        }];
        let request = StreamOptionsRequest {
            request_id: 1,
            scale_permille: 1001,
            fps: 60,
            codec: StreamCodecPreference::Auto,
            display_index: 0,
            bitrate: 10_000_000,
        };
        let err = validate_stream_options(&request, &displays).unwrap_err();
        assert_eq!(err.0, StreamConfigurationErrorCode::UnsupportedDimensions);
        assert!(!err.1.is_empty());
    }

    #[test]
    fn validate_stream_options_rejects_display_index_out_of_range() {
        let displays = vec![HostDisplay {
            index: 0,
            width: 1920,
            height: 1080,
            primary: true,
            name: "Display 1".to_string(),
        }];
        let request = StreamOptionsRequest {
            request_id: 1,
            scale_permille: 1000,
            fps: 60,
            codec: StreamCodecPreference::Auto,
            display_index: 1,
            bitrate: 10_000_000,
        };
        let err = validate_stream_options(&request, &displays).unwrap_err();
        assert_eq!(err.0, StreamConfigurationErrorCode::InvalidRequest);
        assert!(validate_stream_options(&request, &[]).is_err());
    }

    #[test]
    fn downscale_bgra_nearest_halves_dimensions() {
        let sw = 4;
        let sh = 4;
        let dw = 2;
        let dh = 2;
        // Create 4x4 image with 4 distinct colored 2x2 quadrants
        // Top-left: Red (255, 0, 0, 255)
        // Top-right: Green (0, 255, 0, 255)
        // Bottom-left: Blue (0, 0, 255, 255)
        // Bottom-right: White (255, 255, 255, 255)
        let mut src = vec![0u8; (sw * sh * 4) as usize];
        for y in 0..sh {
            for x in 0..sw {
                let idx = ((y * sw + x) * 4) as usize;
                let color = match (x < 2, y < 2) {
                    (true, true) => [0, 0, 255, 255], // Red in BGRA: B=0, G=0, R=255, A=255
                    (false, true) => [0, 255, 0, 255], // Green in BGRA: B=0, G=255, R=0, A=255
                    (true, false) => [255, 0, 0, 255], // Blue in BGRA: B=255, G=0, R=0, A=255
                    (false, false) => [255, 255, 255, 255], // White in BGRA
                };
                src[idx..idx + 4].copy_from_slice(&color);
            }
        }

        let dst = downscale_bgra_nearest(&src, (sw * 4) as usize, sw, sh, dw, dh);
        assert_eq!(dst.len(), (dw * dh * 4) as usize);

        // Pixel (0,0) sampled from (0,0) -> Red
        assert_eq!(&dst[0..4], &[0, 0, 255, 255]);
        // Pixel (1,0) sampled from (2,0) -> Green
        assert_eq!(&dst[4..8], &[0, 255, 0, 255]);
        // Pixel (0,1) sampled from (0,2) -> Blue
        assert_eq!(&dst[8..12], &[255, 0, 0, 255]);
        // Pixel (1,1) sampled from (2,2) -> White
        assert_eq!(&dst[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn stream_options_request_restarts_pipeline_and_replies_active() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x77; 32];
        store
            .save(PairingRecord {
                id: "restart-test".into(),
                name: "client-restart".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let mut config = test_config(store, consent_tx);
        config.display.pixel_width = 1920;
        config.display.pixel_height = 1080;
        let server = HostServer::bind_synthetic(config, 100).unwrap();
        let starts = server
            .synthetic_starts
            .clone()
            .expect("synthetic start counter");
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());

        let client =
            TlsPskClient::new(PskIdentity::pairing("restart-test", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let handshake = Handshake {
            name: "client-restart".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION
                | Capabilities::STREAM_OPTIONS,
            pairing_id: "restart-test".into(),
            session_salt: [0x53; 16],
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();

        // Read HandshakeAck, HostPolicy, HostDisplayList
        let _ack = tcp.read_frame().unwrap();
        let _policy = tcp.read_frame().unwrap();
        let _displays = tcp.read_frame().unwrap();
        let initial = tcp.read_frame().unwrap();
        assert!(
            matches!(ControlMessage::decode(decode_tcp_packet(&initial).1).unwrap(), ControlMessage::StreamOptionsActive(active) if active.codec == StreamCodecPreference::H264)
        );

        assert_eq!(starts.load(Ordering::SeqCst), 1);

        // Send StreamOptionsRequest with scale_permille = 500, fps = 30, codec = H264
        let request = StreamOptionsRequest {
            request_id: 101,
            scale_permille: 500,
            fps: 30,
            codec: StreamCodecPreference::H264,
            display_index: 0,
            bitrate: 0,
        };
        send_tcp_control(&mut tcp, ControlMessage::StreamOptionsRequest(request)).unwrap();

        // Read response
        let resp_frame = tcp.read_frame().unwrap();
        let (hdr, payload) = decode_tcp_packet(&resp_frame);
        assert_eq!(hdr.packet_type, PacketType::Control);
        let resp_msg = ControlMessage::decode(payload).unwrap();
        match resp_msg {
            ControlMessage::StreamOptionsActive(active) => {
                assert_eq!(active.request_id, 101);
                assert_eq!(active.width, 960);
                assert_eq!(active.height, 540);
                assert_eq!(active.fps, 30);
                assert_eq!(active.codec, StreamCodecPreference::H264);
                assert_eq!(active.display_index, 0);
            }
            other => panic!("expected StreamOptionsActive, got {other:?}"),
        }

        // Verify start count is now 2
        assert_eq!(starts.load(Ordering::SeqCst), 2);

        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        server_thread.join().unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn stream_option_capabilities_follow_media_source() {
        let synthetic = SyntheticMediaSource {
            frame_count: 1,
            restart_starts: Arc::default(),
        };
        let windows = WindowsMediaSource {
            display_index: 0,
            fps: 60,
            width: 1920,
            height: 1080,
            bitrate: 10_000_000,
            codec: VideoCodec::H264,
            capture_audio: false,
            desktop_x: 0,
            desktop_y: 0,
        };

        assert!(synthetic.supports_cpu_scaling());
        assert!(synthetic.output_target().is_none());
        assert!(!windows.supports_cpu_scaling());
    }

    #[test]
    fn invalid_stream_options_keep_pipeline_running() {
        let directory = tempdir().unwrap();
        let store = PairingStore::new(directory.path().join("pairing-keys.json"));
        let key = [0x78; 32];
        store
            .save(PairingRecord {
                id: "invalid-opt-test".into(),
                name: "client-invalid".into(),
                key,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let (consent_tx, _) = mpsc::channel();
        let mut config = test_config(store, consent_tx);
        config.display.pixel_width = 1920;
        config.display.pixel_height = 1080;
        let server = HostServer::bind_synthetic(config, 100).unwrap();
        let starts = server
            .synthetic_starts
            .clone()
            .expect("synthetic start counter");
        let tcp_addr = server.tcp_addr().unwrap();
        let server_thread = thread::spawn(move || server.serve_n(1).unwrap());

        let client =
            TlsPskClient::new(PskIdentity::pairing("invalid-opt-test", &key).unwrap()).unwrap();
        let mut tcp = client.connect(tcp_addr).unwrap();
        let handshake = Handshake {
            name: "client-invalid".into(),
            width: 0,
            height: 0,
            scale: 1.0,
            version: PROTOCOL_VERSION,
            capabilities: Capabilities::AUTHENTICATED_UDP_REGISTRATION
                | Capabilities::STREAM_OPTIONS,
            pairing_id: "invalid-opt-test".into(),
            session_salt: [0x54; 16],
        };
        let mut packet = PacketHeader::new(PacketType::Handshake, 0, 0, 0)
            .encode()
            .unwrap();
        packet.extend_from_slice(&handshake.encode().unwrap());
        tcp.write_frame(&packet).unwrap();

        // Read HandshakeAck, HostPolicy, HostDisplayList
        let _ack = tcp.read_frame().unwrap();
        let _policy = tcp.read_frame().unwrap();
        let _displays = tcp.read_frame().unwrap();
        let initial = tcp.read_frame().unwrap();
        assert!(
            matches!(ControlMessage::decode(decode_tcp_packet(&initial).1).unwrap(), ControlMessage::StreamOptionsActive(active) if active.codec == StreamCodecPreference::H264)
        );

        assert_eq!(starts.load(Ordering::SeqCst), 1);

        // Send invalid StreamOptionsRequest with fps = 0
        let request = StreamOptionsRequest {
            request_id: 202,
            scale_permille: 1000,
            fps: 0,
            codec: StreamCodecPreference::Auto,
            display_index: 0,
            bitrate: 0,
        };
        send_tcp_control(&mut tcp, ControlMessage::StreamOptionsRequest(request)).unwrap();

        // Expect StreamConfigReject
        let resp_frame = tcp.read_frame().unwrap();
        let (hdr, payload) = decode_tcp_packet(&resp_frame);
        assert_eq!(hdr.packet_type, PacketType::Control);
        let resp_msg = ControlMessage::decode(payload).unwrap();
        match resp_msg {
            ControlMessage::StreamConfigReject(reject) => {
                assert_eq!(reject.request_id, 202);
                assert_eq!(reject.reason, StreamConfigurationErrorCode::UnsupportedFps);
            }
            other => panic!("expected StreamConfigReject, got {other:?}"),
        }

        // Verify start count remains 1 (pipeline was not restarted)
        assert_eq!(starts.load(Ordering::SeqCst), 1);

        send_tcp_control(&mut tcp, ControlMessage::Disconnect).unwrap();
        server_thread.join().unwrap();
    }
}
