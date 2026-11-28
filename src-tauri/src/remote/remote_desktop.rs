//! Daemon-owned ferryx-rd remote desktop host and its two gateway routes.
//!
//! The host (`ferryx_rd_host::HostServer`) is a blocking, std-thread design, so it runs on one
//! dedicated thread and stops through `serve_with_stop`. It never runs a PIN bootstrap
//! (`bootstrap_pin = None`): a pairing is minted by `POST /api/v1/remote-desktop/pair` for a
//! device the owner already granted Machine scope, and returned inside the authenticated
//! tunnel. `GET /api/v1/remote-desktop/stream` bridges that tunnel to the host's loopback TCP and
//! UDP sockets, so the ferryx-rd TLS-PSK session stays end to end.
//!
//! Every method of [`RemoteDesktopHost`] may block (disk, thread joins); callers run it on a
//! blocking worker.

use crate::daemon::protocol::{RdHostPending, RdHostSession, RdHostState};
use crate::paired_host::client::{RD_DGRAM_ID_HEADER, RD_DGRAM_OPT_IN_HEADER};
use crate::paired_host::direct::datagram::{
    send_datagram_checked, DatagramChannelGuard, DatagramSendOutcome,
};
use crate::remote::auth::{DeviceAccessScope, DeviceInfo, DevicePermission};
use crate::remote::state::RemoteGatewayState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use ferryx_rd_host::guests::{GuestRegistry, HostEvent};
use ferryx_rd_host::{HostConfig, HostServer, PairingRecord, PairingStore};
use futures_util::{SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const PAIR_ROUTE: &str = "/api/v1/remote-desktop/pair";
pub const STREAM_ROUTE: &str = "/api/v1/remote-desktop/stream";
/// Stream frame channel byte: a chunk of the TCP byte stream to the host's control port.
pub const CHANNEL_TCP: u8 = 0x01;
/// Stream frame channel byte: exactly one UDP datagram to or from the host's media port.
pub const CHANNEL_UDP: u8 = 0x02;
/// Largest TCP payload one stream frame carries; keeps a frame under the 64 KiB tunnel cap.
pub const MAX_TCP_CHUNK: usize = 60_000;
/// WebSocket message ceiling for the stream route (one channel byte plus the largest payload).
const MAX_STREAM_MESSAGE: usize = 64 * 1024;
/// Tauri event the GUI re-emits for a pending consent prompt.
pub const CONSENT_EVENT: &str = "rd-host-consent";
/// Tauri event the GUI re-emits whenever sessions or pending prompts change.
pub const STATE_CHANGED_EVENT: &str = "rd-host-state-changed";
/// Bound on how long starting the host (capture probe, encoder setup, bind) may take.
const START_TIMEOUT: Duration = Duration::from_secs(30);
const SETTINGS_FILE: &str = "host-settings.json";
const PAIRINGS_FILE: &str = "host-pairings.json";

pub type RdEventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// A typed failure; `code` reaches the GUI as the IPC error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RdError {
    pub code: &'static str,
    pub message: String,
}

impl RdError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn status(&self) -> StatusCode {
        match self.code {
            "RD_HOST_DISABLED" => StatusCode::CONFLICT,
            "RD_HOST_UNAVAILABLE" => StatusCode::SERVICE_UNAVAILABLE,
            "RD_NOT_FOUND" => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// The pairing the `/pair` route hands a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairGrant {
    pub pairing_id: String,
    pub key_b64: String,
    pub host_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostSettings {
    enabled: bool,
    allow_input: bool,
    allow_clipboard: bool,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_input: true,
            allow_clipboard: true,
        }
    }
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    registry: Arc<GuestRegistry>,
    tcp: SocketAddr,
    udp: SocketAddr,
    host_name: String,
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    settings: HostSettings,
    running: Option<Running>,
    error: Option<String>,
}

pub struct RemoteDesktopHost {
    dir: Option<PathBuf>,
    inner: Mutex<Inner>,
    allow_input: Arc<AtomicBool>,
    allow_clipboard: Arc<AtomicBool>,
    sink: Arc<RwLock<Option<RdEventSink>>>,
    /// Test-only loopback endpoints standing in for a running host (no capture needed).
    #[cfg(test)]
    test_endpoints: Mutex<Option<(SocketAddr, SocketAddr)>>,
}

/// `<Ferryx data dir>/remote-desktop`.
pub fn default_dir() -> Option<PathBuf> {
    crate::worktree::naming::data_dir_with_env(|key| std::env::var_os(key))
        .map(|dir| dir.join("remote-desktop"))
}

/// The pairing id a device's record lives under. Derived, so a device always gets the same
/// record back, and bounded to a TLS-PSK identity-safe alphabet and length.
pub fn pairing_id_for_device(device_id: &str) -> String {
    use sha2::Digest;
    use std::fmt::Write as _;
    let digest = sha2::Sha256::digest(device_id.as_bytes());
    let mut id = String::from("FERRYX-");
    for byte in &digest[..16] {
        let _ = write!(id, "{byte:02X}");
    }
    id
}

/// Returns the device's stored pairing, minting and saving one on first use.
pub fn pairing_for_device(
    store: &PairingStore,
    device_id: &str,
    device_name: &str,
) -> Result<PairingRecord, RdError> {
    let id = pairing_id_for_device(device_id);
    if let Some(record) = store.load(&id).map_err(store_error)? {
        store
            .set_authorization(&record.id, ferryx_rd_proto::PairingTrust::Owner, None)
            .map_err(store_error)?;
        return Ok(record);
    }
    let mut key = [0_u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut key);
    let record = PairingRecord {
        id,
        name: device_name.to_owned(),
        key,
        added_at_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0),
    };
    store.save(record.clone()).map_err(store_error)?;
    store
        .set_authorization(&record.id, ferryx_rd_proto::PairingTrust::Owner, None)
        .map_err(store_error)?;
    Ok(record)
}

fn store_error(error: ferryx_rd_host::session::SessionError) -> RdError {
    RdError::new("IO_ERROR", format!("remote desktop pairing store failed: {error}"))
}

impl RemoteDesktopHost {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let defaults = HostSettings::default();
        Self {
            dir,
            inner: Mutex::new(Inner::default()),
            allow_input: Arc::new(AtomicBool::new(defaults.allow_input)),
            allow_clipboard: Arc::new(AtomicBool::new(defaults.allow_clipboard)),
            sink: Arc::new(RwLock::new(None)),
            #[cfg(test)]
            test_endpoints: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_test_endpoints(&self, tcp: SocketAddr, udp: SocketAddr) {
        *self.test_endpoints.lock() = Some((tcp, udp));
    }

    pub fn set_event_sink(&self, sink: RdEventSink) {
        *self.sink.write() = Some(sink);
    }

    fn dir(&self) -> Result<&PathBuf, RdError> {
        self.dir
            .as_ref()
            .ok_or_else(|| RdError::new("IO_ERROR", "the Ferryx data directory is unavailable"))
    }

    fn store(&self) -> Result<PairingStore, RdError> {
        Ok(PairingStore::new(self.dir()?.join(PAIRINGS_FILE)))
    }

    fn ensure_loaded(&self, inner: &mut Inner) {
        if inner.loaded {
            return;
        }
        inner.loaded = true;
        let Some(dir) = self.dir.as_ref() else {
            return;
        };
        match std::fs::read(dir.join(SETTINGS_FILE)) {
            Ok(bytes) => match serde_json::from_slice::<HostSettings>(&bytes) {
                Ok(settings) => inner.settings = settings,
                Err(error) => tracing::warn!(%error, "ignoring unreadable remote desktop settings"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, "remote desktop settings read failed"),
        }
        self.allow_input
            .store(inner.settings.allow_input, Ordering::SeqCst);
        self.allow_clipboard
            .store(inner.settings.allow_clipboard, Ordering::SeqCst);
    }

    fn save_settings(&self, settings: HostSettings) -> Result<(), RdError> {
        let dir = self.dir()?;
        let io = |error: std::io::Error| {
            RdError::new("IO_ERROR", format!("saving remote desktop settings failed: {error}"))
        };
        std::fs::create_dir_all(dir).map_err(io)?;
        let bytes = serde_json::to_vec_pretty(&settings)
            .map_err(|error| RdError::new("IO_ERROR", error.to_string()))?;
        let temporary = dir.join(format!("{SETTINGS_FILE}.tmp"));
        std::fs::write(&temporary, bytes).map_err(io)?;
        std::fs::rename(&temporary, dir.join(SETTINGS_FILE)).map_err(io)
    }

    /// Starts the host at daemon boot when the owner left it enabled.
    pub fn restore(&self) {
        let mut inner = self.inner.lock();
        self.ensure_loaded(&mut inner);
        if inner.settings.enabled && inner.running.is_none() {
            self.start_locked(&mut inner);
        }
    }

    pub fn state(&self) -> RdHostState {
        let mut inner = self.inner.lock();
        self.ensure_loaded(&mut inner);
        Self::snapshot(&inner)
    }

    fn snapshot(inner: &Inner) -> RdHostState {
        let (sessions, pending) = match &inner.running {
            Some(running) => (
                running
                    .registry
                    .sessions()
                    .into_iter()
                    .map(|session| RdHostSession {
                        session_id: session.id,
                        client_name: session.name,
                    })
                    .collect(),
                running
                    .registry
                    .pending_requests()
                    .into_iter()
                    .map(|request| RdHostPending {
                        request_id: request.id,
                        client_name: request.name,
                    })
                    .collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        RdHostState {
            enabled: inner.settings.enabled,
            running: inner.running.is_some(),
            allow_input: inner.settings.allow_input,
            allow_clipboard: inner.settings.allow_clipboard,
            sessions,
            pending,
            error: inner.error.clone(),
        }
    }

    pub fn set(
        &self,
        enabled: bool,
        allow_input: bool,
        allow_clipboard: bool,
    ) -> Result<RdHostState, RdError> {
        let mut inner = self.inner.lock();
        self.ensure_loaded(&mut inner);
        let settings = HostSettings {
            enabled,
            allow_input,
            allow_clipboard,
        };
        self.save_settings(settings)?;
        inner.settings = settings;
        self.allow_input.store(allow_input, Ordering::SeqCst);
        self.allow_clipboard.store(allow_clipboard, Ordering::SeqCst);
        if enabled {
            if inner.running.is_none() {
                self.start_locked(&mut inner);
            }
        } else {
            Self::stop_locked(&mut inner);
            inner.error = None;
        }
        Ok(Self::snapshot(&inner))
    }

    pub fn respond(&self, request_id: &str, approve: bool) -> Result<RdHostState, RdError> {
        let mut inner = self.inner.lock();
        self.ensure_loaded(&mut inner);
        let answered = inner.running.as_ref().is_some_and(|running| {
            running.registry.respond(
                request_id,
                approve.then_some(ferryx_rd_proto::PairingTrust::Owner),
            )
        });
        if !answered {
            return Err(RdError::new(
                "RD_NOT_FOUND",
                "the remote desktop request is no longer pending",
            ));
        }
        Ok(Self::snapshot(&inner))
    }

    /// The device's pairing, minted on first use. Refused unless the host is enabled and running.
    pub fn pair(&self, device_id: &str, device_name: &str) -> Result<PairGrant, RdError> {
        let host_name = {
            let mut inner = self.inner.lock();
            self.ensure_loaded(&mut inner);
            if !inner.settings.enabled {
                return Err(RdError::new("RD_HOST_DISABLED", "remote desktop hosting is off"));
            }
            if inner.running.is_none() {
                self.start_locked(&mut inner);
            }
            match &inner.running {
                Some(running) => running.host_name.clone(),
                None => {
                    return Err(RdError::new(
                        "RD_HOST_UNAVAILABLE",
                        inner
                            .error
                            .clone()
                            .unwrap_or_else(|| "remote desktop host is not running".into()),
                    ))
                }
            }
        };
        let record = pairing_for_device(&self.store()?, device_id, device_name)?;
        use base64::Engine as _;
        Ok(PairGrant {
            pairing_id: record.id,
            key_b64: base64::engine::general_purpose::STANDARD.encode(record.key),
            host_name,
        })
    }

    /// Mints a local loopback pairing and returns `(grant, tcp_port, udp_port)` for `rd-bridge`.
    pub fn pair_local(&self, device_name: &str) -> Result<(PairGrant, u16, u16), RdError> {
        let (host_name, tcp_port, udp_port) = {
            let mut inner = self.inner.lock();
            self.ensure_loaded(&mut inner);
            if !inner.settings.enabled {
                return Err(RdError::new("RD_HOST_DISABLED", "remote desktop hosting is off"));
            }
            if inner.running.is_none() {
                self.start_locked(&mut inner);
            }
            match &inner.running {
                Some(running) => (running.host_name.clone(), running.tcp.port(), running.udp.port()),
                None => {
                    return Err(RdError::new(
                        "RD_HOST_UNAVAILABLE",
                        inner
                            .error
                            .clone()
                            .unwrap_or_else(|| "remote desktop host is not running".into()),
                    ))
                }
            }
        };
        let label = if device_name.trim().is_empty() {
            "SSH Client"
        } else {
            device_name.trim()
        };
        let record = pairing_for_device(&self.store()?, "ssh-local", label)?;
        use base64::Engine as _;
        Ok((
            PairGrant {
                pairing_id: record.id,
                key_b64: base64::engine::general_purpose::STANDARD.encode(record.key),
                host_name,
            },
            tcp_port,
            udp_port,
        ))
    }

    /// Drops a revoked device's pairing so its key stops authenticating at once.
    pub fn revoke_device(&self, device_id: &str) {
        let Ok(store) = self.store() else {
            return;
        };
        if let Err(error) = store.revoke(&pairing_id_for_device(device_id)) {
            tracing::warn!(%error, "dropping a revoked device's remote desktop pairing failed");
        }
    }

    /// Loopback TCP and UDP endpoints of the running host.
    pub fn endpoints(&self) -> Result<(SocketAddr, SocketAddr), RdError> {
        #[cfg(test)]
        if let Some(endpoints) = *self.test_endpoints.lock() {
            return Ok(endpoints);
        }
        let mut inner = self.inner.lock();
        self.ensure_loaded(&mut inner);
        if !inner.settings.enabled {
            return Err(RdError::new("RD_HOST_DISABLED", "remote desktop hosting is off"));
        }
        if inner.running.is_none() {
            self.start_locked(&mut inner);
        }
        let running = inner.running.as_ref().ok_or_else(|| {
            RdError::new("RD_HOST_UNAVAILABLE", "remote desktop host is not running")
        })?;
        let loopback = |addr: SocketAddr| SocketAddr::from((Ipv4Addr::LOCALHOST, addr.port()));
        Ok((loopback(running.tcp), loopback(running.udp)))
    }

    fn start_locked(&self, inner: &mut Inner) {
        match self.store().and_then(|store| {
            spawn_host(
                store,
                Arc::clone(&self.allow_input),
                Arc::clone(&self.allow_clipboard),
                Arc::clone(&self.sink),
            )
        }) {
            Ok(running) => {
                inner.running = Some(running);
                inner.error = None;
            }
            Err(error) => {
                tracing::warn!(error = %error.message, "remote desktop host failed to start");
                inner.error = Some(error.message);
            }
        }
    }

    fn stop_locked(inner: &mut Inner) {
        if let Some(mut running) = inner.running.take() {
            running.stop.store(true, Ordering::SeqCst);
            running.registry.kick_all();
            if let Some(thread) = running.thread.take() {
                if thread.join().is_err() {
                    tracing::warn!("remote desktop host thread panicked");
                }
            }
        }
    }
}

impl Drop for RemoteDesktopHost {
    fn drop(&mut self) {
        Self::stop_locked(&mut self.inner.lock());
    }
}

#[cfg(target_os = "macos")]
fn host_config(store: PairingStore) -> Result<HostConfig, String> {
    HostConfig::macos_default(None, store).map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn host_config(store: PairingStore) -> Result<HostConfig, String> {
    ferryx_rd_host::windows_default_blocking(None, store).map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn host_config(store: PairingStore) -> Result<HostConfig, String> {
    HostConfig::linux_default(None, store, None).map_err(|error| error.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn host_config(_store: PairingStore) -> Result<HostConfig, String> {
    Err("remote desktop hosting is not supported on this platform".into())
}

/// Binds on the default ports so a LAN client can reach the host directly, falling back to
/// ephemeral ports when another host (a standalone ferryx-rd install) already holds them.
fn bind_host(config: HostConfig) -> Result<HostServer, String> {
    match HostServer::bind_unadvertised(config.clone()) {
        Ok(server) => Ok(server),
        Err(ferryx_rd_host::session::SessionError::Io(error))
            if error.kind() == std::io::ErrorKind::AddrInUse =>
        {
            let mut fallback = config;
            fallback.tcp_addr.set_port(0);
            fallback.udp_addr.set_port(0);
            HostServer::bind_unadvertised(fallback).map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

struct Started {
    registry: Arc<GuestRegistry>,
    tcp: SocketAddr,
    udp: SocketAddr,
    host_name: String,
}

fn spawn_host(
    store: PairingStore,
    allow_input: Arc<AtomicBool>,
    allow_clipboard: Arc<AtomicBool>,
    sink: Arc<RwLock<Option<RdEventSink>>>,
) -> Result<Running, RdError> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Started, String>>(1);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("ferryx-rd-host".into())
        .spawn(move || {
            let server = host_config(store).and_then(|mut config| {
                config.allow_input = allow_input;
                config.allow_clipboard = allow_clipboard;
                // Media may ride direct QUIC datagrams; keep every packet inside that budget.
                config.max_udp_payload_bytes =
                    ferryx_rd_host::session::QUIC_TUNNEL_MAX_UDP_PAYLOAD_BYTES;
                let host_name = config.host_name.clone();
                let server = bind_host(config)?;
                let tcp = server.tcp_addr().map_err(|error| error.to_string())?;
                let udp = server.udp_addr().map_err(|error| error.to_string())?;
                Ok((server, host_name, tcp, udp))
            });
            let (server, host_name, tcp, udp) = match server {
                Ok(started) => started,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            let registry = server.guest_registry();
            spawn_event_forwarder(Arc::clone(&registry), registry.subscribe(), sink);
            if tx
                .send(Ok(Started {
                    registry,
                    tcp,
                    udp,
                    host_name,
                }))
                .is_err()
            {
                return;
            }
            if let Err(error) = server.serve_with_stop(thread_stop) {
                tracing::warn!(%error, "remote desktop host stopped");
            }
        })
        .map_err(|error| RdError::new("RD_HOST_UNAVAILABLE", error.to_string()))?;
    match rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(started)) => Ok(Running {
            stop,
            thread: Some(thread),
            registry: started.registry,
            tcp: started.tcp,
            udp: started.udp,
            host_name: started.host_name,
        }),
        Ok(Err(message)) => {
            let _ = thread.join();
            Err(RdError::new("RD_HOST_UNAVAILABLE", message))
        }
        Err(_) => {
            // A start that outlives the bound is abandoned: the flag makes it exit as soon as
            // it reaches the accept loop instead of serving with nobody able to stop it.
            stop.store(true, Ordering::SeqCst);
            Err(RdError::new(
                "RD_HOST_UNAVAILABLE",
                "remote desktop host did not start in time",
            ))
        }
    }
}

fn spawn_event_forwarder(
    registry: Arc<GuestRegistry>,
    events: std::sync::mpsc::Receiver<HostEvent>,
    sink: Arc<RwLock<Option<RdEventSink>>>,
) {
    spawn_event_forwarder_with_responder(events, sink, move |request_id| {
        let _ = registry.respond(request_id, Some(ferryx_rd_proto::PairingTrust::Owner));
    });
}

fn spawn_event_forwarder_with_responder(
    events: std::sync::mpsc::Receiver<HostEvent>,
    sink: Arc<RwLock<Option<RdEventSink>>>,
    auto_approve: impl Fn(&str) + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("ferryx-rd-host-events".into())
        .spawn(move || {
            while let Ok(event) = events.recv() {
                if let HostEvent::ConsentRequested(request) = &event {
                    auto_approve(&request.id);
                }
                let Some(sink) = sink.read().clone() else {
                    continue;
                };
                sink(STATE_CHANGED_EVENT, serde_json::json!({}));
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "remote desktop event forwarder did not start");
    }
}

// ---- stream framing ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamChannel {
    Tcp,
    Udp,
}

pub fn encode_stream_frame(channel: StreamChannel, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(match channel {
        StreamChannel::Tcp => CHANNEL_TCP,
        StreamChannel::Udp => CHANNEL_UDP,
    });
    frame.extend_from_slice(payload);
    frame
}

/// `None` for an empty frame, an unknown channel or an oversized TCP chunk.
pub fn decode_stream_frame(frame: &[u8]) -> Option<(StreamChannel, &[u8])> {
    let (&channel, payload) = frame.split_first()?;
    match channel {
        CHANNEL_TCP if payload.len() <= MAX_TCP_CHUNK => Some((StreamChannel::Tcp, payload)),
        CHANNEL_UDP => Some((StreamChannel::Udp, payload)),
        _ => None,
    }
}

pub async fn write_stdio_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    channel: StreamChannel,
    payload: &[u8],
) -> std::io::Result<()> {
    let total = 1usize + payload.len();
    if total > MAX_STREAM_MESSAGE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "stdio bridge frame exceeds limit",
        ));
    }
    let mut header = [0_u8; 5];
    header[..4].copy_from_slice(&(total as u32).to_be_bytes());
    header[4] = match channel {
        StreamChannel::Tcp => CHANNEL_TCP,
        StreamChannel::Udp => CHANNEL_UDP,
    };
    writer.write_all(&header).await?;
    writer.write_all(payload).await?;
    writer.flush().await
}

pub async fn read_stdio_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<(StreamChannel, Vec<u8>)>> {
    let mut len_buf = [0_u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let total = u32::from_be_bytes(len_buf) as usize;
    if total == 0 || total > MAX_STREAM_MESSAGE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid stdio bridge frame length",
        ));
    }
    let mut frame = vec![0_u8; total];
    reader.read_exact(&mut frame).await?;
    let Some((channel, payload)) = decode_stream_frame(&frame) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid stdio bridge channel",
        ));
    };
    Ok(Some((channel, payload.to_vec())))
}

pub async fn run_stdio_bridge(tcp_port: u16, udp_port: u16) -> std::io::Result<()> {
    let tcp_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, tcp_port));
    let udp_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, udp_port));
    let tcp = tokio::net::TcpStream::connect(tcp_addr).await?;
    tcp.set_nodelay(true)?;
    let udp = tokio::net::UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    udp.connect(udp_addr).await?;
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut tcp_buf = vec![0_u8; MAX_TCP_CHUNK];
    let mut udp_buf = vec![0_u8; 64 * 1024];
    loop {
        tokio::select! {
            incoming = read_stdio_frame(&mut stdin) => match incoming? {
                Some((StreamChannel::Tcp, payload)) => tcp_write.write_all(&payload).await?,
                Some((StreamChannel::Udp, payload)) => {
                    let _ = udp.send(&payload).await?;
                }
                None => return Ok(()),
            },
            read = tcp_read.read(&mut tcp_buf) => {
                let n = read?;
                if n == 0 {
                    return Ok(());
                }
                write_stdio_frame(&mut stdout, StreamChannel::Tcp, &tcp_buf[..n]).await?;
            }
            received = udp.recv(&mut udp_buf) => {
                let n = received?;
                write_stdio_frame(&mut stdout, StreamChannel::Udp, &udp_buf[..n]).await?;
            }
        }
    }
}

/// Direct QUIC datagram media state for one stream: the connection, the minted channel id, the
/// channel's receiver and the guard that unregisters it from the mux when dropped.
type DatagramBridge = (
    quinn::Connection,
    u64,
    tokio::sync::mpsc::Receiver<bytes::Bytes>,
    DatagramChannelGuard,
);

/// The next datagram for this stream; pends forever once there is no (or no longer a) channel.
async fn next_datagram(
    rx: &mut Option<tokio::sync::mpsc::Receiver<bytes::Bytes>>,
) -> Option<bytes::Bytes> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn bridge(
    socket: WebSocket,
    tcp_addr: SocketAddr,
    udp_addr: SocketAddr,
    datagrams: Option<DatagramBridge>,
) -> std::io::Result<()> {
    let tcp = tokio::net::TcpStream::connect(tcp_addr).await?;
    tcp.set_nodelay(true)?;
    let udp = tokio::net::UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    udp.connect(udp_addr).await?;
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let (mut ws_tx, mut ws_rx) = socket.split();
    // Bound after the socket halves so it drops before them: when the bridge ends (exit or
    // revocation) the channel leaves the mux before the peer can observe the close.
    let (dgram_send, mut dgram_rx, _dgram_guard) = match datagrams {
        Some((connection, channel_id, rx, guard)) => {
            (Some((connection, channel_id)), Some(rx), Some(guard))
        }
        None => (None, None, None),
    };
    let mut tcp_buf = vec![0_u8; MAX_TCP_CHUNK];
    let mut udp_buf = vec![0_u8; 64 * 1024];
    let mut dgram_open = dgram_rx.is_some();
    loop {
        tokio::select! {
            incoming = next_datagram(&mut dgram_rx), if dgram_open => match incoming {
                Some(payload) => {
                    udp.send(&payload).await?;
                }
                // The mux was cleared (the QUIC reader ended); WebSocket frames still work.
                None => dgram_open = false,
            },
            message = ws_rx.next() => match message {
                Some(Ok(Message::Binary(data))) => match decode_stream_frame(&data) {
                    Some((StreamChannel::Tcp, payload)) => tcp_write.write_all(payload).await?,
                    Some((StreamChannel::Udp, payload)) => {
                        udp.send(payload).await?;
                    }
                    None => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "invalid remote desktop stream frame",
                        ))
                    }
                },
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(std::io::Error::other(error)),
            },
            read = tcp_read.read(&mut tcp_buf) => {
                let n = read?;
                if n == 0 {
                    let _ = ws_tx.send(Message::Close(None)).await;
                    return Ok(());
                }
                let frame = encode_stream_frame(StreamChannel::Tcp, &tcp_buf[..n]);
                ws_tx.send(Message::Binary(frame.into())).await.map_err(std::io::Error::other)?;
            }
            received = udp.recv(&mut udp_buf) => {
                let n = received?;
                if let Some((connection, channel_id)) = &dgram_send {
                    if send_datagram_checked(connection, *channel_id, &udp_buf[..n])
                        == DatagramSendOutcome::Sent
                    {
                        continue;
                    }
                }
                let frame = encode_stream_frame(StreamChannel::Udp, &udp_buf[..n]);
                ws_tx.send(Message::Binary(frame.into())).await.map_err(std::io::Error::other)?;
            }
        }
    }
}

// ---- gateway routes ----

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "code": code, "message": message })),
    )
        .into_response()
}

fn rd_error_response(error: &RdError) -> Response {
    error_response(error.status(), error.code, &error.message)
}

/// The same bearer check every privileged gateway route runs, then Machine scope with Control.
async fn authorize(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
) -> Result<DeviceInfo, Box<Response>> {
    let refuse = |status, code, message| Box::new(error_response(status, code, message));
    let token = super::server::extract_token(headers)
        .ok_or_else(|| refuse(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "missing auth token"))?;
    let auth = Arc::clone(&state.auth_manager);
    let device = tokio::task::spawn_blocking(move || auth.validate_token(&token))
        .await
        .map_err(|_| {
            refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "authorization unavailable",
            )
        })?
        .map_err(|_| refuse(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "invalid or revoked token"))?;
    if device.access_scope != DeviceAccessScope::Machine
        || device.permission != DevicePermission::Control
    {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "MACHINE_ACCESS_REQUIRED",
            "remote desktop needs a Machine grant",
        ));
    }
    Ok(device)
}

async fn pair_handler(State(state): State<Arc<RemoteGatewayState>>, headers: HeaderMap) -> Response {
    let device = match authorize(&state, &headers).await {
        Ok(device) => device,
        Err(response) => return *response,
    };
    let host = Arc::clone(&state.remote_desktop);
    match tokio::task::spawn_blocking(move || host.pair(&device.id, &device.name)).await {
        Ok(Ok(grant)) => Json(grant).into_response(),
        Ok(Err(error)) => rd_error_response(&error),
        Err(_) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_ERROR",
            "remote desktop pairing task failed",
        ),
    }
}

async fn stream_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<RemoteGatewayState>>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
) -> Response {
    // Full authorization runs before anything touches the datagram registry.
    let device = match authorize(&state, &headers).await {
        Ok(device) => device,
        Err(response) => return *response,
    };
    let host = Arc::clone(&state.remote_desktop);
    let (tcp, udp) = match tokio::task::spawn_blocking(move || host.endpoints()).await {
        Ok(Ok(endpoints)) => endpoints,
        Ok(Err(error)) => return rd_error_response(&error),
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "remote desktop task failed",
            )
        }
    };
    let Ok(mut revocation) = state.auth_manager.device_revocation(&device.id) else {
        return error_response(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "invalid or revoked token");
    };
    let datagrams = datagram_channel(&state, peer, &headers);
    let channel_id = datagrams.as_ref().map(|(_, channel_id, _, _)| *channel_id);
    let mut response = ws
        .max_message_size(MAX_STREAM_MESSAGE)
        .max_frame_size(MAX_STREAM_MESSAGE)
        .on_upgrade(move |socket| async move {
            tokio::select! {
                biased;
                _ = revocation.wait_for(|revoked| *revoked) => {}
                result = bridge(socket, tcp, udp, datagrams) => {
                    if let Err(error) = result {
                        tracing::debug!(%error, "remote desktop stream ended");
                    }
                }
            }
        });
    if let Some(channel_id) = channel_id {
        if let Ok(value) = HeaderValue::from_str(&format!("{channel_id:016x}")) {
            response
                .headers_mut()
                .insert(HeaderName::from_static(RD_DGRAM_ID_HEADER), value);
        }
    }
    response
}

/// A datagram channel for this stream when the (already authorized) caller opted in AND its
/// TCP peer is a live direct QUIC bridge stream whose connection supports datagrams. Anything
/// else (relay, plain TCP, a forged opt-in, a peer without datagram support) gets `None` and
/// stays on WebSocket UDP frames.
fn datagram_channel(
    state: &RemoteGatewayState,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    headers: &HeaderMap,
) -> Option<DatagramBridge> {
    let opted_in = headers
        .get(RD_DGRAM_OPT_IN_HEADER)
        .is_some_and(|value| value.as_bytes() == b"1");
    if !opted_in {
        return None;
    }
    let axum::Extension(ConnectInfo(peer_addr)) = peer?;
    let direct = state.direct_datagram_peers.lookup(&peer_addr)?;
    direct.connection.max_datagram_size()?;
    let channel_id = rand::random::<u64>().max(1);
    let (rx, guard) = direct.mux.register(channel_id);
    Some((direct.connection, channel_id, rx, guard))
}

/// The remote desktop gateway routes, merged into the daemon gateway router.
pub fn router(state: Arc<RemoteGatewayState>) -> Router {
    Router::new()
        .route(PAIR_ROUTE, post(pair_handler))
        .route(STREAM_ROUTE, get(stream_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_frames_round_trip_on_both_channels() {
        let tcp = encode_stream_frame(StreamChannel::Tcp, b"hello");
        assert_eq!(tcp[0], CHANNEL_TCP);
        assert_eq!(decode_stream_frame(&tcp), Some((StreamChannel::Tcp, &b"hello"[..])));
        let udp = encode_stream_frame(StreamChannel::Udp, &[9, 8, 7]);
        assert_eq!(udp, vec![CHANNEL_UDP, 9, 8, 7]);
        assert_eq!(decode_stream_frame(&udp), Some((StreamChannel::Udp, &[9_u8, 8, 7][..])));
        let largest = vec![0xAB; MAX_TCP_CHUNK];
        let frame = encode_stream_frame(StreamChannel::Tcp, &largest);
        assert!(frame.len() <= MAX_STREAM_MESSAGE);
        assert_eq!(decode_stream_frame(&frame), Some((StreamChannel::Tcp, &largest[..])));
    }

    #[test]
    fn malformed_stream_frames_are_refused() {
        assert_eq!(decode_stream_frame(&[]), None);
        assert_eq!(decode_stream_frame(&[0x03, 1, 2]), None);
        let oversized = encode_stream_frame(StreamChannel::Tcp, &vec![0; MAX_TCP_CHUNK + 1]);
        assert_eq!(decode_stream_frame(&oversized), None);
    }

    #[tokio::test]
    async fn stdio_frames_round_trip_and_reject_oversized() {
        let mut wire = Vec::new();
        write_stdio_frame(&mut wire, StreamChannel::Tcp, b"tcp-bytes")
            .await
            .expect("write tcp");
        write_stdio_frame(&mut wire, StreamChannel::Udp, &[7, 8, 9])
            .await
            .expect("write udp");
        let mut cursor = std::io::Cursor::new(wire);
        assert_eq!(
            read_stdio_frame(&mut cursor).await.expect("read tcp"),
            Some((StreamChannel::Tcp, b"tcp-bytes".to_vec()))
        );
        assert_eq!(
            read_stdio_frame(&mut cursor).await.expect("read udp"),
            Some((StreamChannel::Udp, vec![7, 8, 9]))
        );
        assert_eq!(read_stdio_frame(&mut cursor).await.expect("eof"), None);
    }

    #[test]
    fn a_device_gets_the_same_pairing_record_every_time() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = PairingStore::new(dir.path().join(PAIRINGS_FILE));
        let first = pairing_for_device(&store, "device-a", "Laptop").expect("pair");
        let again = pairing_for_device(&store, "device-a", "Renamed").expect("pair again");
        assert_eq!(first, again);
        let other = pairing_for_device(&store, "device-b", "Phone").expect("pair other");
        assert_ne!(first.id, other.id);
        assert_ne!(first.key, other.key);
        // A fresh store over the same file reads the same record back.
        let reopened = PairingStore::new(dir.path().join(PAIRINGS_FILE));
        assert_eq!(reopened.load(&first.id).expect("load"), Some(first.clone()));
        assert!(first.id.starts_with("FERRYX-") && first.id.len() == "FERRYX-".len() + 32);
    }

    #[test]
    fn host_defaults_to_enabled_and_pairing_grants_owner_trust() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = RemoteDesktopHost::new(Some(dir.path().to_path_buf()));
        let state = host.state();
        assert!(state.enabled && state.allow_input && state.allow_clipboard);

        let store = PairingStore::new(dir.path().join(PAIRINGS_FILE));
        let record = pairing_for_device(&store, "device-a", "Laptop").expect("pair");
        let (trust, permissions) = store.authorization(&record.id).expect("authorization");
        assert_eq!(trust, ferryx_rd_proto::PairingTrust::Owner);
        assert_eq!(permissions, None);
    }

    #[test]
    fn consent_requests_are_auto_approved_without_gui_prompt() {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (approved_tx, approved_rx) = std::sync::mpsc::sync_channel(1);
        let emitted = Arc::new(Mutex::new(Vec::<String>::new()));
        let emitted_sink = Arc::clone(&emitted);
        let sink: Arc<RwLock<Option<RdEventSink>>> = Arc::new(RwLock::new(Some(Arc::new(
            move |name: &str, _payload: serde_json::Value| {
                emitted_sink.lock().push(name.to_owned());
            },
        ))));
        spawn_event_forwarder_with_responder(event_rx, sink, move |request_id| {
            let _ = approved_tx.send(request_id.to_owned());
        });

        event_tx
            .send(HostEvent::ConsentRequested(
                ferryx_rd_host::guests::PendingRequest {
                    id: "req-unattended".into(),
                    name: "Remote Mac".into(),
                    expires_at: std::time::Instant::now() + Duration::from_secs(5),
                },
            ))
            .expect("send consent event");
        let approved_id = approved_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("auto-approval should fire without waiting on GUI");
        assert_eq!(approved_id, "req-unattended");
        drop(event_tx);
        assert!(!emitted.lock().iter().any(|name| name == CONSENT_EVENT));
    }

    #[test]
    fn a_disabled_host_refuses_pairing_and_streams_and_revocation_drops_the_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = RemoteDesktopHost::new(Some(dir.path().to_path_buf()));
        host.set(false, true, true).expect("disable host");
        assert_eq!(host.pair("device-a", "Laptop").unwrap_err().code, "RD_HOST_DISABLED");
        assert_eq!(host.endpoints().unwrap_err().code, "RD_HOST_DISABLED");
        let state = host.state();
        assert!(!state.enabled && !state.running && state.sessions.is_empty());
        assert_eq!(host.respond("missing", true).unwrap_err().code, "RD_NOT_FOUND");

        let store = PairingStore::new(dir.path().join(PAIRINGS_FILE));
        let record = pairing_for_device(&store, "device-a", "Laptop").expect("pair");
        host.revoke_device("device-a");
        assert_eq!(store.load(&record.id).expect("load"), None);
    }

    mod datagram_route {
        use super::super::*;
        use crate::paired_host::direct::datagram::{
            decode_datagram_frame, spawn_datagram_reader, DatagramMux, DirectDatagramPeer,
        };
        use crate::paired_host::direct::{create_quic_endpoint, generate_ephemeral_cert};
        use crate::remote::auth::{DeviceAccessScope, DevicePermission};
        use ferryx_rd_host::session::QUIC_TUNNEL_MAX_UDP_PAYLOAD_BYTES;
        use tokio::net::{TcpListener, TcpStream, UdpSocket};
        use tokio_tungstenite::tungstenite::{
            self, client::IntoClientRequest, handshake::client::Response as WsResponse,
            Message as WsMessage,
        };
        use tokio_tungstenite::WebSocketStream;

        const BOUND: Duration = Duration::from_secs(15);

        /// The real stream route behind `ConnectInfo`, with loopback sockets standing in for
        /// the running host so no screen capture is needed.
        struct Gateway {
            _dir: tempfile::TempDir,
            state: Arc<RemoteGatewayState>,
            addr: SocketAddr,
            host_tcp: TcpListener,
            host_udp: UdpSocket,
            token: String,
            device_id: String,
            server: tokio::task::JoinHandle<()>,
        }

        impl Drop for Gateway {
            fn drop(&mut self) {
                self.server.abort();
            }
        }

        async fn gateway() -> Gateway {
            let dir = tempfile::tempdir().expect("tempdir");
            let state = Arc::new(RemoteGatewayState::new_with_paths(
                Arc::new(crate::terminal::TerminalService::default()),
                crate::worktree::WorkspaceRegistry::new(),
                Some(dir.path().join("remote-config.json")),
                Some(dir.path().join("remote-auth.json")),
            ));
            let host_tcp = TcpListener::bind("127.0.0.1:0").await.expect("host tcp");
            let host_udp = UdpSocket::bind("127.0.0.1:0").await.expect("host udp");
            state.remote_desktop.set_test_endpoints(
                host_tcp.local_addr().expect("host tcp addr"),
                host_udp.local_addr().expect("host udp addr"),
            );
            let code = state
                .auth_manager
                .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
                .expect("machine pairing code");
            let (token, device) = state
                .auth_manager
                .exchange_pairing_code(&code, "rd datagram test")
                .expect("exchange");
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("gateway");
            let addr = listener.local_addr().expect("gateway addr");
            let app = router(Arc::clone(&state));
            let server = tokio::spawn(async move {
                let _ = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await;
            });
            Gateway {
                _dir: dir,
                state,
                addr,
                host_tcp,
                host_udp,
                token,
                device_id: device.id,
                server,
            }
        }

        async fn upgrade(
            tcp: TcpStream,
            gateway: SocketAddr,
            token: Option<&str>,
            opt_in: bool,
        ) -> Result<(WebSocketStream<TcpStream>, WsResponse), tungstenite::Error> {
            let mut request = format!("ws://{gateway}{STREAM_ROUTE}")
                .into_client_request()
                .expect("request");
            if let Some(token) = token {
                request.headers_mut().insert(
                    "authorization",
                    format!("Bearer {token}").parse().expect("bearer"),
                );
            }
            if opt_in {
                request
                    .headers_mut()
                    .insert(RD_DGRAM_OPT_IN_HEADER, "1".parse().expect("opt-in"));
            }
            tokio_tungstenite::client_async_with_config(request, tcp, None).await
        }

        struct QuicPair {
            server_endpoint: quinn::Endpoint,
            client_endpoint: quinn::Endpoint,
            /// The host's end, as the direct bridge registers it.
            host: quinn::Connection,
            client: quinn::Connection,
        }

        impl Drop for QuicPair {
            fn drop(&mut self) {
                self.server_endpoint.close(0u32.into(), b"done");
                self.client_endpoint.close(0u32.into(), b"done");
            }
        }

        async fn quic_pair() -> QuicPair {
            let server_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("server udp");
            let client_sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("client udp");
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
            let (client, host) = tokio::try_join!(connect, accept).expect("quic handshake");
            QuicPair {
                server_endpoint,
                client_endpoint,
                host,
                client,
            }
        }

        fn channel_id(response: &WsResponse) -> Option<u64> {
            let value = response.headers().get(RD_DGRAM_ID_HEADER)?.to_str().ok()?;
            assert_eq!(value.len(), 16, "zero-padded hex id: {value}");
            u64::from_str_radix(value, 16).ok()
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn registered_direct_peer_moves_udp_onto_quic_datagrams() {
            tokio::time::timeout(BOUND, async {
                let gw = gateway().await;
                let quic = quic_pair().await;
                let mux = DatagramMux::new();
                let reader = spawn_datagram_reader(quic.host.clone(), Arc::clone(&mux));
                let tcp = TcpStream::connect(gw.addr).await.expect("dial gateway");
                let _peer = gw.state.direct_datagram_peers.register(
                    tcp.local_addr().expect("local addr"),
                    DirectDatagramPeer {
                        connection: quic.host.clone(),
                        mux: Arc::clone(&mux),
                    },
                );
                let (mut ws, response) = upgrade(tcp, gw.addr, Some(gw.token.as_str()), true)
                    .await
                    .expect("upgrade");
                let channel = channel_id(&response).expect("101 carries the datagram id");
                assert_ne!(channel, 0);
                assert_eq!(mux.len(), 1);
                let (mut host_tcp, _) = gw.host_tcp.accept().await.expect("bridge dials host");

                // Client -> host loopback UDP over a QUIC datagram.
                assert_eq!(
                    send_datagram_checked(&quic.client, channel, b"from-client"),
                    DatagramSendOutcome::Sent
                );
                let mut buf = vec![0_u8; 2048];
                let (n, bridge_udp) = gw.host_udp.recv_from(&mut buf).await.expect("host udp");
                assert_eq!(&buf[..n], b"from-client");

                // Host -> client: full-budget packets ride datagrams, in lockstep.
                for seq in 0..5_u8 {
                    let payload = vec![seq; QUIC_TUNNEL_MAX_UDP_PAYLOAD_BYTES];
                    gw.host_udp.send_to(&payload, bridge_udp).await.expect("host send");
                    let frame = quic.client.read_datagram().await.expect("datagram");
                    let (id, got) = decode_datagram_frame(frame).expect("valid frame");
                    assert_eq!(id, channel);
                    assert_eq!(&got[..], &payload[..]);
                }

                // No UDP went over the WebSocket: the next frame is this TCP marker.
                host_tcp.write_all(b"marker").await.expect("host tcp write");
                let message = ws.next().await.expect("open").expect("frame");
                assert_eq!(
                    &message.into_data()[..],
                    &encode_stream_frame(StreamChannel::Tcp, b"marker")[..]
                );
                reader.abort();
            })
            .await
            .expect("test bounded");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn forged_opt_in_from_unregistered_peer_stays_on_websocket_udp() {
            tokio::time::timeout(BOUND, async {
                let gw = gateway().await;
                let quic = quic_pair().await;
                let mux = DatagramMux::new();
                // A live direct peer exists, but under another loopback address.
                let _other = gw.state.direct_datagram_peers.register(
                    SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
                    DirectDatagramPeer {
                        connection: quic.host.clone(),
                        mux: Arc::clone(&mux),
                    },
                );
                let tcp = TcpStream::connect(gw.addr).await.expect("dial gateway");
                let (mut ws, response) = upgrade(tcp, gw.addr, Some(gw.token.as_str()), true)
                    .await
                    .expect("upgrade");
                assert!(response.headers().get(RD_DGRAM_ID_HEADER).is_none());
                assert_eq!(mux.len(), 0);
                let (_host_tcp, _) = gw.host_tcp.accept().await.expect("bridge dials host");

                ws.send(WsMessage::Binary(
                    encode_stream_frame(StreamChannel::Udp, b"ping").into(),
                ))
                .await
                .expect("ws send");
                let mut buf = vec![0_u8; 2048];
                let (n, bridge_udp) = gw.host_udp.recv_from(&mut buf).await.expect("host udp");
                assert_eq!(&buf[..n], b"ping");
                gw.host_udp.send_to(b"pong", bridge_udp).await.expect("host send");
                let message = ws.next().await.expect("open").expect("frame");
                assert_eq!(
                    &message.into_data()[..],
                    &encode_stream_frame(StreamChannel::Udp, b"pong")[..]
                );
            })
            .await
            .expect("test bounded");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn revocation_closes_the_bridge_and_unregisters_the_channel() {
            tokio::time::timeout(BOUND, async {
                let gw = gateway().await;
                let quic = quic_pair().await;
                let mux = DatagramMux::new();
                let tcp = TcpStream::connect(gw.addr).await.expect("dial gateway");
                let _peer = gw.state.direct_datagram_peers.register(
                    tcp.local_addr().expect("local addr"),
                    DirectDatagramPeer {
                        connection: quic.host.clone(),
                        mux: Arc::clone(&mux),
                    },
                );
                let (mut ws, response) = upgrade(tcp, gw.addr, Some(gw.token.as_str()), true)
                    .await
                    .expect("upgrade");
                assert!(channel_id(&response).is_some());
                assert_eq!(mux.len(), 1);
                let (mut host_tcp, _) = gw.host_tcp.accept().await.expect("bridge dials host");
                // One TCP round trip proves the bridge reached its loop.
                ws.send(WsMessage::Binary(
                    encode_stream_frame(StreamChannel::Tcp, b"hi").into(),
                ))
                .await
                .expect("ws send");
                let mut hi = [0_u8; 2];
                host_tcp.read_exact(&mut hi).await.expect("host tcp read");
                assert_eq!(&hi, b"hi");

                assert!(gw
                    .state
                    .auth_manager
                    .revoke_device(&gw.device_id)
                    .expect("revoke"));
                // The channel guard drops before the socket, so once the close is visible
                // the channel is already gone.
                loop {
                    match ws.next().await {
                        Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => break,
                        Some(Ok(_)) => {}
                    }
                }
                assert_eq!(mux.len(), 0);
            })
            .await
            .expect("test bounded");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn unauthorized_opt_in_never_allocates_a_channel() {
            tokio::time::timeout(BOUND, async {
                let gw = gateway().await;
                let view_code = gw.state.auth_manager.create_pairing_code(DevicePermission::View);
                let (mirror_token, mirror) = gw
                    .state
                    .auth_manager
                    .exchange_pairing_code(&view_code, "rd mirror")
                    .expect("exchange");
                assert_eq!(mirror.access_scope, DeviceAccessScope::Mirror);
                let quic = quic_pair().await;
                let mux = DatagramMux::new();
                for (token, status) in [
                    (None, 401_u16),
                    (Some("not-a-token"), 401),
                    (Some(mirror_token.as_str()), 403),
                ] {
                    let tcp = TcpStream::connect(gw.addr).await.expect("dial gateway");
                    let _peer = gw.state.direct_datagram_peers.register(
                        tcp.local_addr().expect("local addr"),
                        DirectDatagramPeer {
                            connection: quic.host.clone(),
                            mux: Arc::clone(&mux),
                        },
                    );
                    let refused = upgrade(tcp, gw.addr, token, true)
                        .await
                        .expect_err("unauthorized upgrade is refused");
                    let tungstenite::Error::Http(response) = refused else {
                        panic!("expected an HTTP refusal");
                    };
                    assert_eq!(response.status().as_u16(), status);
                    assert!(response.headers().get(RD_DGRAM_ID_HEADER).is_none());
                    assert_eq!(mux.len(), 0, "no channel before authorization");
                }
            })
            .await
            .expect("test bounded");
        }
    }
}
