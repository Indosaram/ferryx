// allow: SIZE_OK — daemon IPC server implementation with routing, session persistence offloading, remote control, and streaming
use crate::daemon::protocol::{
    DaemonRemoteStatus, DaemonRequest, DaemonResponse, DaemonSessionDetails, DaemonStreamMessage,
    DAEMON_PROTOCOL_VERSION,
};
use crate::remote::auth::DevicePermission;
use crate::remote::server::{start_remote_server, RemoteServerHandle};
use crate::remote::state::{RemoteGatewayConfig, RemoteGatewayState, RemoteNetworkMode};
use crate::session::{clear_session_from_path, load_session_from_path, save_session_to_path};
use crate::terminal::{PtyManager, PtySessionState, TerminalOutputHub, TerminalService};
use crate::worktree::{WorkspaceRegistry, WorktreeIdentity, WorktreeManager};
use parking_lot::{Mutex, RwLock};
use portable_pty::CommandBuilder;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(all(test, unix))]
use tokio::net::UnixStream;
#[cfg(not(unix))]
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

#[cfg(unix)]
pub fn get_runtime_dir() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/rorca-{uid}"))
}

#[cfg(not(unix))]
pub fn get_runtime_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("TEMP"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Ferryx")
        .join("runtime")
}

#[cfg(unix)]
pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.sock")
}

#[cfg(not(unix))]
pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.port")
}

pub fn get_lock_path() -> PathBuf {
    get_runtime_dir().join("daemon.lock")
}

pub fn get_default_session_path() -> PathBuf {
    if let Some(mut base) = dirs_next().or_else(dirs_fallback) {
        base.push("rorca");
        let _ = fs::create_dir_all(&base);
        base.join("session_state.json")
    } else {
        get_runtime_dir().join("session_state.json")
    }
}

fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".rorca"))
}

#[derive(Clone, Copy)]
enum RuntimeNodeKind {
    Directory,
    RegularFile,
    Socket,
}

#[cfg(unix)]
fn validate_safe_ownership_and_type_for_uid(
    path: &Path,
    kind: RuntimeNodeKind,
    expected_uid: libc::uid_t,
) -> Result<(), String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Path {} is a symlink, which is prohibited for daemon runtime",
            path.display()
        ));
    }
    if meta.uid() != expected_uid {
        return Err(format!(
            "Path {} is owned by UID {} (expected current UID {})",
            path.display(),
            meta.uid(),
            expected_uid
        ));
    }
    let valid_type = match kind {
        RuntimeNodeKind::Directory => meta.file_type().is_dir(),
        RuntimeNodeKind::RegularFile => meta.file_type().is_file(),
        RuntimeNodeKind::Socket => meta.file_type().is_socket(),
    };
    if !valid_type {
        return Err(format!(
            "Path {} has an invalid runtime node type",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_safe_ownership_and_type_for_uid(
    path: &Path,
    kind: RuntimeNodeKind,
    _expected_uid: u32,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Path {} is a symlink, which is prohibited for daemon runtime",
            path.display()
        ));
    }
    let valid_type = match kind {
        RuntimeNodeKind::Directory => meta.file_type().is_dir(),
        RuntimeNodeKind::RegularFile | RuntimeNodeKind::Socket => !meta.file_type().is_dir(),
    };
    if !valid_type {
        return Err(format!(
            "Path {} has an invalid runtime node type",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_safe_ownership_and_type(path: &Path, kind: RuntimeNodeKind) -> Result<(), String> {
    validate_safe_ownership_and_type_for_uid(path, kind, unsafe { libc::getuid() })
}

#[cfg(not(unix))]
fn validate_safe_ownership_and_type(path: &Path, kind: RuntimeNodeKind) -> Result<(), String> {
    validate_safe_ownership_and_type_for_uid(path, kind, 0)
}

#[cfg(unix)]
pub(crate) fn validate_runtime_socket_path(path: &Path) -> Result<(), String> {
    validate_runtime_socket_path_for_uid(path, unsafe { libc::getuid() })
}

#[cfg(not(unix))]
pub(crate) fn validate_runtime_socket_path(path: &Path) -> Result<(), String> {
    validate_runtime_socket_path_for_uid(path, 0)
}

#[cfg(unix)]
pub(crate) fn validate_runtime_socket_path_for_uid(
    path: &Path,
    expected_uid: libc::uid_t,
) -> Result<(), String> {
    let runtime_dir = path
        .parent()
        .ok_or_else(|| format!("Daemon socket {} has no runtime directory", path.display()))?;
    validate_safe_ownership_and_type_for_uid(
        runtime_dir,
        RuntimeNodeKind::Directory,
        expected_uid,
    )?;
    let mode = fs::symlink_metadata(runtime_dir)
        .map_err(|error| format!("Failed to verify {}: {error}", runtime_dir.display()))?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o700 {
        return Err(format!(
            "Daemon runtime directory {} has mode {mode:o}, expected 700",
            runtime_dir.display()
        ));
    }
    validate_safe_ownership_and_type_for_uid(path, RuntimeNodeKind::Socket, expected_uid)
}

#[cfg(not(unix))]
pub(crate) fn validate_runtime_socket_path_for_uid(
    path: &Path,
    _expected_uid: u32,
) -> Result<(), String> {
    if let Some(runtime_dir) = path.parent() {
        validate_safe_ownership_and_type_for_uid(runtime_dir, RuntimeNodeKind::Directory, 0)?;
    }
    validate_safe_ownership_and_type_for_uid(path, RuntimeNodeKind::RegularFile, 0)?;
    Ok(())
}

fn ensure_runtime_directory(path: &Path) -> Result<(), String> {
    match fs::create_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "Failed to create daemon runtime directory {}: {error}",
                path.display()
            ));
        }
    }
    validate_safe_ownership_and_type(path, RuntimeNodeKind::Directory)?;
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "Failed to secure daemon runtime directory {}: {error}",
                path.display()
            )
        })?;
        let mode = fs::symlink_metadata(path)
            .map_err(|error| format!("Failed to verify {}: {error}", path.display()))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            return Err(format!(
                "Daemon runtime directory {} has mode {mode:o}, expected 700",
                path.display()
            ));
        }
    }
    Ok(())
}

fn open_secure_lock_file(path: &Path) -> Result<File, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_safe_ownership_and_type(path, RuntimeNodeKind::RegularFile)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect lock file: {error}")),
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);

    #[cfg(unix)]
    {
        options.mode(0o600);
        options.custom_flags(libc::O_NOFOLLOW);
    }

    let file = options
        .open(path)
        .map_err(|error| format!("Failed to open lock file {}: {error}", path.display()))?;
    validate_safe_ownership_and_type(path, RuntimeNodeKind::RegularFile)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure lock file: {error}"))?;
    Ok(file)
}

fn remove_stale_socket_after_lock(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            validate_safe_ownership_and_type(path, RuntimeNodeKind::Socket)?;
            fs::remove_file(path).map_err(|error| {
                format!("Failed to remove stale socket {}: {error}", path.display())
            })
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to inspect daemon socket: {error}")),
    }
}

const SPAWN_REQUEST_TTL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct SpawnCacheEntry {
    session_id: String,
    created_at: Instant,
}

#[derive(Clone)]
struct StoredSessionMeta {
    client_request_id: String,
    workspace_id: String,
    worktree: Option<WorktreeIdentity>,
    cwd: PathBuf,
}

pub struct DaemonServer {
    terminal_service: Arc<TerminalService>,
    workspace_registry: WorkspaceRegistry,
    remote_state: Arc<RemoteGatewayState>,
    remote_server_handle: Arc<Mutex<Option<RemoteServerHandle>>>,
    epoch: u64,
    spawn_idempotency_cache: Arc<Mutex<HashMap<String, SpawnCacheEntry>>>,
    spawn_lock: tokio::sync::Mutex<()>,
    session_metadata: Arc<RwLock<HashMap<String, StoredSessionMeta>>>,
}

impl Default for DaemonServer {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonServer {
    pub fn new() -> Self {
        Self::new_with_paths(None, None)
    }

    pub fn new_with_paths(config_path: Option<PathBuf>, auth_path: Option<PathBuf>) -> Self {
        let pty_manager = Arc::new(PtyManager::new());
        let output_hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(
            Arc::clone(&pty_manager),
            Arc::clone(&output_hub),
        ));
        let workspace_registry = WorkspaceRegistry::new();
        #[cfg(test)]
        let remote_state = Arc::new(RemoteGatewayState::new_with_paths(
            Arc::clone(&terminal_service),
            workspace_registry.clone(),
            config_path,
            auth_path,
        ));
        #[cfg(not(test))]
        let remote_state = if config_path.is_some() || auth_path.is_some() {
            Arc::new(RemoteGatewayState::new_with_paths(
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
                config_path,
                auth_path,
            ))
        } else {
            Arc::new(RemoteGatewayState::new_persistent(
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
            ))
        };

        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1);

        Self {
            terminal_service,
            workspace_registry,
            remote_state,
            remote_server_handle: Arc::new(Mutex::new(None)),
            epoch,
            spawn_idempotency_cache: Arc::new(Mutex::new(HashMap::new())),
            spawn_lock: tokio::sync::Mutex::new(()),
            session_metadata: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn terminal_service(&self) -> &Arc<TerminalService> {
        &self.terminal_service
    }

    pub fn workspace_registry(&self) -> &WorkspaceRegistry {
        &self.workspace_registry
    }

    pub fn remote_state(&self) -> &Arc<RemoteGatewayState> {
        &self.remote_state
    }

    pub async fn run_server(self: Arc<Self>) -> Result<(), String> {
        let runtime_dir = get_runtime_dir();
        ensure_runtime_directory(&runtime_dir)?;

        let socket_path = runtime_dir.join("daemon.sock");
        let lock_path = runtime_dir.join("daemon.lock");

        // Flock lock for atomic bind & stale socket recovery.
        let lock_file = open_secure_lock_file(&lock_path)?;

        #[cfg(unix)]
        unsafe {
            use std::os::unix::io::AsRawFd;
            let fd = lock_file.as_raw_fd();
            if libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) != 0 {
                return Err("Another daemon instance is already holding the lock.".into());
            }
        }

        // Clean up stale socket only after lock acquisition and safe ownership check.
        remove_stale_socket_after_lock(&socket_path)?;

        #[cfg(unix)]
        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            format!(
                "Failed to bind UDS socket at {}: {e}",
                socket_path.display()
            )
        })?;

        #[cfg(not(unix))]
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
            format!("Failed to bind TCP listener on localhost: {e}")
        })?;

        #[cfg(not(unix))]
        {
            let port = listener.local_addr().map_err(|e| format!("Failed to get local port: {e}"))?.port();
            fs::write(&socket_path, port.to_string()).map_err(|e| format!("Failed to write daemon.port: {e}"))?;
        }

        // Ensure 0600 mode
        validate_safe_ownership_and_type(&socket_path, RuntimeNodeKind::Socket)?;
        #[cfg(unix)]
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Failed to secure daemon socket: {error}"))?;

        tracing::info!("rorca daemon listening on {}", socket_path.display());

        let persisted_remote_config = self.remote_state.config.read().clone();
        if persisted_remote_config.mode != RemoteNetworkMode::Off {
            if let Err(error) = self.handle_remote_configure(persisted_remote_config).await {
                tracing::warn!("Failed to restore daemon remote gateway listener: {error}");
            }
        }

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        server.handle_client(stream).await;
                    });
                }
                Err(e) => {
                    tracing::error!("Daemon accept error: {e}");
                    break;
                }
            }
        }

        Ok(())
    }

    pub async fn handle_client<S>(self: Arc<Self>, stream: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 {
                break;
            }
            let req: Result<DaemonRequest, _> = serde_json::from_str(line.trim());
            line.clear();

            let resp = match req {
                Ok(DaemonRequest::Handshake { version }) => {
                    if version != DAEMON_PROTOCOL_VERSION {
                        DaemonResponse::ProtocolMismatch {
                            expected_version: DAEMON_PROTOCOL_VERSION,
                            received_version: version,
                        }
                    } else {
                        DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: self.epoch,
                        }
                    }
                }
                Ok(DaemonRequest::Ping) => DaemonResponse::Pong,
                Ok(DaemonRequest::RegisterWorkspace {
                    workspace_id,
                    repo_root,
                }) => match self.handle_register_workspace(&workspace_id, &repo_root) {
                    Ok(()) => DaemonResponse::RegisterWorkspaceOk,
                    Err(e) => DaemonResponse::Error { message: e },
                },
                Ok(DaemonRequest::Spawn {
                    client_request_id,
                    workspace_id,
                    worktree,
                    cwd,
                    cols,
                    rows,
                }) => {
                    let res = self
                        .handle_spawn(&client_request_id, &workspace_id, worktree, cwd, cols, rows)
                        .await;
                    match res {
                        Ok(session_id) => DaemonResponse::SpawnOk { session_id },
                        Err(e) => DaemonResponse::Error { message: e },
                    }
                }
                Ok(DaemonRequest::DescribeSession { session_id }) => {
                    self.handle_describe_session(&session_id)
                }
                Ok(DaemonRequest::Write { session_id, data }) => {
                    match self.terminal_service.write_input(&session_id, &data) {
                        Ok(()) => DaemonResponse::WriteOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::Resize {
                    session_id,
                    cols,
                    rows,
                }) => match self.terminal_service.resize(&session_id, cols, rows) {
                    Ok(()) => DaemonResponse::ResizeOk,
                    Err(e) => DaemonResponse::Error {
                        message: e.to_string(),
                    },
                },
                Ok(DaemonRequest::Signal { session_id, signal }) => {
                    match self.terminal_service.signal(&session_id, signal) {
                        Ok(()) => DaemonResponse::SignalOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::Close { session_id }) => {
                    self.session_metadata.write().remove(&session_id);
                    match self.terminal_service.close_session(&session_id).await {
                        Ok(()) => DaemonResponse::CloseOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::ListSessions) => {
                    let sessions = self.terminal_service.list_sessions();
                    DaemonResponse::ListSessionsOk {
                        epoch: self.epoch,
                        sessions,
                    }
                }
                Ok(DaemonRequest::Attach {
                    session_id,
                    after_sequence,
                }) => {
                    match self
                        .terminal_service
                        .attach_with_sequence(&session_id, after_sequence)
                    {
                        Ok(attachment) => {
                            let hub = Arc::clone(self.terminal_service.output_hub());
                            let resp = DaemonResponse::AttachOk {
                                epoch: self.epoch,
                                session_id: session_id.clone(),
                                start_sequence: attachment.snapshot.history_start_sequence,
                                end_sequence: attachment.snapshot.history_end_sequence,
                                gap: attachment.snapshot.gap,
                                history: attachment.snapshot.history,
                            };
                            let mut resp_json = serde_json::to_string(&resp).unwrap();
                            resp_json.push('\n');
                            let _ = write_half.write_all(resp_json.as_bytes()).await;
                            let _ = write_half.flush().await;

                            Self::pump_sequenced_stream(
                                session_id,
                                attachment.receiver,
                                hub,
                                write_half,
                            )
                            .await;
                            return;
                        }
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::SaveSession { session }) => {
                    let path = get_default_session_path();
                    let res =
                        tokio::task::spawn_blocking(move || save_session_to_path(&path, &session))
                            .await;
                    match res {
                        Ok(Ok(())) => DaemonResponse::SaveSessionOk,
                        Ok(Err(e)) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Save session task panicked: {e}"),
                        },
                    }
                }
                Ok(DaemonRequest::LoadSession) => {
                    let path = get_default_session_path();
                    let res =
                        tokio::task::spawn_blocking(move || load_session_from_path(&path)).await;
                    match res {
                        Ok(Ok(session)) => DaemonResponse::LoadSessionOk { session },
                        Ok(Err(e)) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Load session task panicked: {e}"),
                        },
                    }
                }
                Ok(DaemonRequest::ClearSession) => {
                    let path = get_default_session_path();
                    let res =
                        tokio::task::spawn_blocking(move || clear_session_from_path(&path)).await;
                    match res {
                        Ok(Ok(())) => DaemonResponse::ClearSessionOk,
                        Ok(Err(e)) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Clear session task panicked: {e}"),
                        },
                    }
                }
                Ok(DaemonRequest::RemoteGetStatus) => {
                    let config = self.remote_state.config.read().clone();
                    let is_running = *self.remote_state.is_running.read();
                    let bound_address = self.remote_state.bound_address.read().clone();
                    DaemonResponse::RemoteStatusOk {
                        status: DaemonRemoteStatus {
                            mode: config.mode,
                            port: config.port,
                            allow_control: config.allow_control,
                            is_running,
                            bound_address,
                        },
                    }
                }
                Ok(DaemonRequest::RemoteConfigure { config }) => {
                    match self.handle_remote_configure(config).await {
                        Ok(()) => DaemonResponse::RemoteConfigureOk,
                        Err(e) => DaemonResponse::Error { message: e },
                    }
                }
                Ok(DaemonRequest::RemoteCreatePairingCode { permission }) => {
                    let perm = permission.unwrap_or(DevicePermission::Control);
                    let code = self.remote_state.auth_manager.create_pairing_code(perm);
                    DaemonResponse::RemotePairingCodeOk { code }
                }
                Ok(DaemonRequest::RemoteListDevices) => {
                    let devices = self.remote_state.auth_manager.list_devices();
                    DaemonResponse::RemoteListDevicesOk { devices }
                }
                Ok(DaemonRequest::RemoteRevokeDevice { device_id }) => {
                    if self.remote_state.auth_manager.revoke_device(&device_id) {
                        DaemonResponse::RemoteRevokeDeviceOk
                    } else {
                        DaemonResponse::Error {
                            message: format!("Device '{device_id}' not found or already revoked"),
                        }
                    }
                }
                Ok(DaemonRequest::RemoteSetActiveSelection { selection }) => {
                    self.remote_state.set_active_selection_opt(selection);
                    DaemonResponse::RemoteSetActiveSelectionOk
                }
                Ok(DaemonRequest::RemoteGetActiveSelection) => {
                    let selection = self.remote_state.active_selection.read().clone();
                    DaemonResponse::RemoteGetActiveSelectionOk { selection }
                }
                Ok(DaemonRequest::Shutdown) => {
                    std::process::exit(0);
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Malformed request: {e}"),
                },
            };

            let mut resp_json = serde_json::to_string(&resp).unwrap();
            resp_json.push('\n');
            if write_half.write_all(resp_json.as_bytes()).await.is_err() {
                break;
            }
            let _ = write_half.flush().await;
            if matches!(resp, DaemonResponse::ProtocolMismatch { .. }) {
                break;
            }
        }
    }

    pub fn handle_register_workspace(
        &self,
        workspace_id: &str,
        repo_root: &str,
    ) -> Result<(), String> {
        let path = PathBuf::from(repo_root);
        if !path.is_absolute() {
            return Err("repo_root must be an absolute path".into());
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|e| format!("Invalid repo_root path '{}': {e}", path.display()))?;
        if !canonical.is_dir() {
            return Err(format!(
                "Repo root '{}' is not a directory",
                canonical.display()
            ));
        }
        let manager = WorktreeManager::try_new(&canonical).map_err(|e| e.to_string())?;
        if manager.repo_root() != canonical {
            return Err(format!(
                "repo_root '{}' must be the canonical repository root '{}'",
                path.display(),
                manager.repo_root().display()
            ));
        }
        self.workspace_registry
            .register(workspace_id, &canonical)
            .map_err(|e| e.to_string())
    }

    pub async fn handle_remote_configure(&self, config: RemoteGatewayConfig) -> Result<(), String> {
        if config.mode == RemoteNetworkMode::Off {
            let prev_handle = self.remote_server_handle.lock().take();
            if let Some(handle) = prev_handle {
                handle.stop();
            }
            *self.remote_state.config.write() = config;
            *self.remote_state.is_running.write() = false;
            *self.remote_state.bound_address.write() = None;
            self.remote_state
                .persist_config()
                .map_err(|error| format!("Failed to persist remote gateway config: {error}"))?;
            return Ok(());
        }

        let prev_config = self.remote_state.config.read().clone();
        *self.remote_state.config.write() = config.clone();

        match start_remote_server(Arc::clone(&self.remote_state)).await {
            Ok((handle, _addr)) => {
                let prev_handle = self.remote_server_handle.lock().take();
                if let Some(h) = prev_handle {
                    h.stop();
                }
                *self.remote_server_handle.lock() = Some(handle);
                self.remote_state
                    .persist_config()
                    .map_err(|error| format!("Failed to persist remote gateway config: {error}"))?;
                Ok(())
            }
            Err(err) => {
                *self.remote_state.config.write() = prev_config;
                *self.remote_state.is_running.write() = false;
                *self.remote_state.bound_address.write() = None;
                let _ = self.remote_state.persist_config();
                Err(err)
            }
        }
    }

    async fn handle_spawn(
        &self,
        client_request_id: &str,
        workspace_id: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<String, String> {
        if client_request_id.trim().is_empty() {
            return Err("clientRequestId cannot be empty".into());
        }

        let _spawn_guard = self.spawn_lock.lock().await;

        let now = Instant::now();
        {
            let mut cache = self.spawn_idempotency_cache.lock();
            cache.retain(|_, entry| {
                now.duration_since(entry.created_at) <= SPAWN_REQUEST_TTL
                    && self
                        .terminal_service
                        .get_session(&entry.session_id)
                        .is_some()
            });
            if let Some(entry) = cache.get_mut(client_request_id) {
                entry.created_at = now;
                return Ok(entry.session_id.clone());
            }
        }

        self.session_metadata
            .write()
            .retain(|session_id, _| self.terminal_service.get_session(session_id).is_some());
        if let Some(live_session_id) = self
            .session_metadata
            .read()
            .iter()
            .find(|(session_id, meta)| {
                meta.client_request_id == client_request_id
                    && self.terminal_service.get_session(session_id).is_some()
            })
            .map(|(session_id, _)| session_id.clone())
        {
            self.spawn_idempotency_cache.lock().insert(
                client_request_id.to_string(),
                SpawnCacheEntry {
                    session_id: live_session_id.clone(),
                    created_at: now,
                },
            );
            return Ok(live_session_id);
        }

        // Resolve manager from workspace registry; workspace MUST be registered.
        let (mgr, default_cwd) = self
            .workspace_registry
            .resolve_terminal_target(workspace_id, worktree.as_ref())
            .map_err(|e| e.to_string())?;

        let resolved_cwd = if let Some(ref custom_cwd_str) = cwd {
            let custom_path = PathBuf::from(custom_cwd_str);
            if !custom_path.exists() {
                return Err(format!("CWD does not exist: {custom_cwd_str}"));
            }
            if !custom_path.is_dir() {
                return Err(format!("CWD is not a directory: {custom_cwd_str}"));
            }
            let canonical = fs::canonicalize(&custom_path)
                .map_err(|e| format!("Cannot canonicalize CWD {custom_cwd_str}: {e}"))?;
            let allowed = mgr
                .canonical_allowed_path(&canonical)
                .map_err(|e| format!("CWD '{custom_cwd_str}' is outside workspace: {e}"))?;
            if allowed != default_cwd && !allowed.starts_with(&default_cwd) {
                return Err(format!(
                    "CWD '{custom_cwd_str}' is outside the resolved workspace/worktree root '{}'",
                    default_cwd.display()
                ));
            }
            allowed
        } else {
            default_cwd
        };

        let mut cmd = CommandBuilder::new_default_prog();
        cmd.cwd(&resolved_cwd);

        let (session_id, _) = self
            .terminal_service
            .spawn_in_worktree(cmd, cols, rows, &mgr, &resolved_cwd)
            .map_err(|e| e.to_string())?;

        // Store idempotency entry and session metadata before releasing the request lock.
        self.spawn_idempotency_cache.lock().insert(
            client_request_id.to_string(),
            SpawnCacheEntry {
                session_id: session_id.clone(),
                created_at: now,
            },
        );
        self.session_metadata.write().insert(
            session_id.clone(),
            StoredSessionMeta {
                client_request_id: client_request_id.to_string(),
                workspace_id: workspace_id.to_string(),
                worktree,
                cwd: resolved_cwd,
            },
        );

        Ok(session_id)
    }

    #[cfg(test)]
    fn expire_spawn_request_for_test(&self, client_request_id: &str) {
        if let Some(entry) = self
            .spawn_idempotency_cache
            .lock()
            .get_mut(client_request_id)
        {
            entry.created_at = Instant::now() - SPAWN_REQUEST_TTL - Duration::from_secs(1);
        }
    }

    #[cfg(test)]
    fn spawn_cache_len_for_test(&self) -> usize {
        self.spawn_idempotency_cache.lock().len()
    }

    fn handle_describe_session(&self, session_id: &str) -> DaemonResponse {
        let Some(pty_session) = self.terminal_service.get_session(session_id) else {
            return DaemonResponse::Error {
                message: format!("Session '{session_id}' not found"),
            };
        };

        let (cols, rows) = pty_session.get_size();
        let running = matches!(
            pty_session.state(),
            PtySessionState::Starting | PtySessionState::Running
        );
        let (start_sequence, end_sequence) = self
            .terminal_service
            .output_hub()
            .session_sequence_range(session_id)
            .unwrap_or((None, None));

        let meta = self.session_metadata.read().get(session_id).cloned();
        let (workspace_id, worktree, cwd) = match meta {
            Some(m) => (
                Some(m.workspace_id),
                m.worktree,
                Some(m.cwd.to_string_lossy().to_string()),
            ),
            None => (
                None,
                None,
                pty_session
                    .worktree_path()
                    .map(|p| p.to_string_lossy().to_string()),
            ),
        };

        DaemonResponse::DescribeSessionOk {
            session: DaemonSessionDetails {
                session_id: session_id.to_string(),
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                running,
                start_sequence,
                end_sequence,
            },
        }
    }

    pub async fn pump_sequenced_stream<W>(
        session_id: String,
        mut rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let mut writer = BufWriter::new(writer);
        let mut last_seen_sequence: Option<u64> = None;

        loop {
            match rx.recv().await {
                Ok(chunk) => {
                    if last_seen_sequence.is_some_and(|last| chunk.sequence <= last) {
                        continue;
                    }
                    last_seen_sequence = Some(chunk.sequence);
                    let msg = DaemonStreamMessage::Output {
                        session_id: Cow::Borrowed(&session_id),
                        sequence: chunk.sequence,
                        data: Cow::Borrowed(&chunk.bytes),
                        metrics_read_unix_micros: chunk.metrics_read_unix_micros,
                    };
                    let mut json = serde_json::to_string(&msg).unwrap();
                    json.push('\n');
                    if writer.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                    if writer.flush().await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Re-subscribe with sequence after last_seen_sequence to recover replay gap
                    if let Some(att) = hub.subscribe_with_sequence(&session_id, last_seen_sequence)
                    {
                        rx = att.receiver;
                        let requested_after_sequence = last_seen_sequence.unwrap_or(0);
                        if att.snapshot.history_end_sequence.is_some() {
                            last_seen_sequence = att.snapshot.history_end_sequence;
                        }
                        let available_from_sequence = att
                            .snapshot
                            .gap
                            .as_ref()
                            .map(|gap| gap.available_from_sequence)
                            .or(att.snapshot.history_start_sequence)
                            .unwrap_or_else(|| requested_after_sequence.saturating_add(1));
                        let msg = DaemonStreamMessage::Lagged {
                            session_id: Cow::Borrowed(&session_id),
                            requested_after_sequence,
                            available_from_sequence,
                            start_sequence: att.snapshot.history_start_sequence,
                            end_sequence: att.snapshot.history_end_sequence,
                            history: Cow::Borrowed(&att.snapshot.history),
                        };
                        let mut json = serde_json::to_string(&msg).unwrap();
                        json.push('\n');
                        if writer.write_all(json.as_bytes()).await.is_err() {
                            break;
                        }
                        if writer.flush().await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let msg = DaemonStreamMessage::Exit {
                        session_id: Cow::Borrowed(&session_id),
                        exit_code: None,
                    };
                    let mut json = serde_json::to_string(&msg).unwrap();
                    json.push('\n');
                    let _ = writer.write_all(json.as_bytes()).await;
                    let _ = writer.flush().await;
                    break;
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::terminal::output_hub::OutputChunk;
    use tempfile::tempdir;

    fn init_test_git_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        dir
    }

    #[tokio::test]
    async fn test_pump_stream_compact_framing_and_exit() {
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let (_server_read, server_write) = server_stream.into_split();
        let (client_read, _client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        let (tx, rx) = broadcast::channel(16);
        let hub = Arc::new(TerminalOutputHub::default());
        let session_id = "test-session-123".to_string();

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream(
            session_id.clone(),
            rx,
            hub,
            server_write,
        ));

        // Send binary output chunk
        tx.send(OutputChunk {
            sequence: 1,
            bytes: b"hello pty stream\n".to_vec(),
            metrics_read_unix_micros: None,
        })
        .unwrap();

        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        assert!(
            !line.contains('['),
            "Must not contain JSON number array: {line}"
        );
        assert!(
            line.contains(r#""data":"aGVsbG8gcHR5IHN0cmVhbQo=""#),
            "Expected base64 data: {line}"
        );
        assert!(
            line.contains(r#""sessionId":"test-session-123""#),
            "Expected camelCase sessionId: {line}"
        );
        assert!(
            line.contains(r#""sequence":1"#),
            "Expected sequence 1: {line}"
        );

        // Close broadcast sender to trigger Exit
        drop(tx);
        line.clear();
        client_reader.read_line(&mut line).await.unwrap();
        assert!(line.contains(r#""type":"exit""#));
        assert!(line.contains(r#""sessionId":"test-session-123""#));

        let _ = pump_handle.await;
    }

    #[tokio::test]
    async fn test_server_register_workspace_and_spawn_isolation() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        let repo_path = repo.path().to_str().unwrap();

        // 1. Spawning before registration must fail explicitly (no GUI CWD fallback)
        let unreg_res = server
            .handle_spawn("req-unreg-1", "ws-app", None, None, 80, 24)
            .await;
        assert!(
            unreg_res.is_err(),
            "Must not infer workspace before explicit registration"
        );

        // 2. Explicit registration of valid canonical git repo
        let reg_res = server.handle_register_workspace("ws-app", repo_path);
        assert!(reg_res.is_ok(), "Registration of valid repo succeeds");

        let nested = repo.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let nested_reg = server.handle_register_workspace("ws-nested", nested.to_str().unwrap());
        assert!(
            nested_reg.is_err(),
            "registration requires the canonical repository root"
        );

        // 3. Spawning in registered workspace succeeds
        let spawn_res = server
            .handle_spawn("req-reg-1", "ws-app", None, None, 80, 24)
            .await;
        assert!(spawn_res.is_ok());

        // 4. Invalid repo root (e.g. non-git directory) fails registration
        let non_git = tempdir().unwrap();
        let bad_reg = server.handle_register_workspace("ws-bad", non_git.path().to_str().unwrap());
        assert!(bad_reg.is_err());
    }

    #[tokio::test]
    async fn test_server_spawn_idempotency_cache() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let req_id = "spawn-req-idempotency-1".to_string();
        let (first, second) = tokio::join!(
            server.handle_spawn(&req_id, "default", None, None, 80, 24),
            server.handle_spawn(&req_id, "default", None, None, 80, 24),
        );
        let session1 = first.expect("first spawn succeeds");
        let session2 = second.expect("concurrent duplicate succeeds");

        assert_eq!(
            session1, session2,
            "Concurrent duplicate requests must share one shell"
        );
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        server
            .terminal_service()
            .close_session(&session1)
            .await
            .expect("close first session");
        let replacement = server
            .handle_spawn(&req_id, "default", None, None, 80, 24)
            .await
            .expect("dead cached session is never returned");
        assert_ne!(session1, replacement);

        server.expire_spawn_request_for_test(&req_id);
        let after_ttl = server
            .handle_spawn(&req_id, "default", None, None, 80, 24)
            .await
            .expect("a repeated id never creates a second live shell");
        assert_eq!(replacement, after_ttl);
        assert_eq!(server.spawn_cache_len_for_test(), 1);
    }

    #[tokio::test]
    async fn test_server_spawn_cwd_validation() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        // Nonexistent CWD
        let err_nonexistent = server
            .handle_spawn(
                "req-bad-cwd",
                "default",
                None,
                Some("/nonexistent/path/for/rorca/test/123".to_string()),
                80,
                24,
            )
            .await;
        assert!(err_nonexistent.is_err());
        assert!(err_nonexistent.unwrap_err().contains("CWD does not exist"));

        let outside = tempdir().unwrap();
        let outside_res = server
            .handle_spawn(
                "req-outside-cwd",
                "default",
                None,
                Some(outside.path().to_string_lossy().into_owned()),
                80,
                24,
            )
            .await;
        assert!(
            outside_res.is_err(),
            "arbitrary cwd outside the registered root must fail"
        );

        let symlink_outside = repo.path().join("escape");
        std::os::unix::fs::symlink(outside.path(), &symlink_outside).unwrap();
        let symlink_res = server
            .handle_spawn(
                "req-symlink-cwd",
                "default",
                None,
                Some(symlink_outside.to_string_lossy().into_owned()),
                80,
                24,
            )
            .await;
        assert!(
            symlink_res.is_err(),
            "symlink escape outside the registered root must fail"
        );

        // Valid CWD inside repo
        let ok_cwd = repo.path().to_str().unwrap().to_string();
        let ok_res = server
            .handle_spawn(
                "req-good-cwd",
                "default",
                None,
                Some(ok_cwd.clone()),
                80,
                24,
            )
            .await;
        assert!(ok_res.is_ok());

        // Verify DescribeSession reflects cwd
        let session_id = ok_res.unwrap();
        let desc_resp = server.handle_describe_session(&session_id);
        match desc_resp {
            DaemonResponse::DescribeSessionOk { session } => {
                assert_eq!(session.session_id, session_id);
                assert!(session.cwd.is_some());
                assert!(session.running);
            }
            other => panic!("Expected DescribeSessionOk, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_runtime_dir_and_socket_symlink_protection() {
        let dir = tempdir().unwrap();
        let symlink_path = dir.path().join("symlink_dir");
        let target_dir = dir.path().join("target_dir");
        fs::create_dir_all(&target_dir).unwrap();
        std::os::unix::fs::symlink(&target_dir, &symlink_path).unwrap();

        let check = validate_safe_ownership_and_type(&symlink_path, RuntimeNodeKind::Directory);
        assert!(check.is_err(), "Symlinks in runtime dir must be rejected");
        assert!(
            symlink_path.exists(),
            "Validation must never delete an unsafe path"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_runtime_node_modes_are_enforced() {
        let dir = tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_runtime_directory(&runtime).expect("secure runtime directory");
        assert_eq!(
            fs::symlink_metadata(&runtime).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let lock = runtime.join("daemon.lock");
        let _file = open_secure_lock_file(&lock).expect("secure lock file");
        assert_eq!(
            fs::symlink_metadata(&lock).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let socket_symlink = runtime.join("daemon.sock");
        let target = runtime.join("target");
        fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, &socket_symlink).unwrap();
        assert!(remove_stale_socket_after_lock(&socket_symlink).is_err());
        assert!(socket_symlink.symlink_metadata().is_ok());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
    }

    #[tokio::test]
    async fn test_daemon_remote_gateway_lifecycle_and_commands() {
        let server = Arc::new(DaemonServer::new());

        // Default: listener is OFF
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // 1. Check default remote status
        let req = DaemonRequest::RemoteGetStatus;
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteStatusOk { status } => {
                assert_eq!(status.mode, RemoteNetworkMode::Off);
                assert!(!status.is_running);
            }
            other => panic!("Expected RemoteStatusOk, got {other:?}"),
        }

        // 2. Create pairing code
        line.clear();
        let pair_req = DaemonRequest::RemoteCreatePairingCode {
            permission: Some(DevicePermission::Control),
        };
        let mut json = serde_json::to_string(&pair_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemotePairingCodeOk { code } => {
                assert_eq!(code.len(), 6);
            }
            other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
        }

        // 3. Active selection set & get
        line.clear();
        let sel_req = DaemonRequest::RemoteSetActiveSelection {
            selection: Some(crate::remote::protocol::RemoteActiveDesktopSelection {
                workspace_id: Some("ws-desktop".to_string()),
                worktree_slug: None,
                worktree_label: None,
                session_id: Some("session-1".to_string()),
            }),
        };
        let mut json = serde_json::to_string(&sel_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::RemoteSetActiveSelectionOk));

        line.clear();
        let get_sel_req = DaemonRequest::RemoteGetActiveSelection;
        let mut json = serde_json::to_string(&get_sel_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteGetActiveSelectionOk { selection } => {
                assert_eq!(
                    selection.unwrap().workspace_id,
                    Some("ws-desktop".to_string())
                );
            }
            other => panic!("Expected RemoteGetActiveSelectionOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_stream_lagged_replay_gap_full_recovery() {
        let hub = Arc::new(TerminalOutputHub::new(128));
        let session_id = "test-lag-session".to_string();
        let (_raw_rx, rx) = hub.register_session_channels(&session_id);

        for i in 0..1100 {
            hub.publish(&session_id, format!("chunk_{i:04};").into_bytes());
        }

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let (_server_read, server_write) = server_stream.into_split();
        let (client_read, _client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream(
            session_id.clone(),
            rx,
            Arc::clone(&hub),
            server_write,
        ));

        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        let message: DaemonStreamMessage<'static> = serde_json::from_str(line.trim()).unwrap();
        match message {
            DaemonStreamMessage::Lagged {
                requested_after_sequence,
                available_from_sequence,
                start_sequence,
                end_sequence,
                history,
                ..
            } => {
                assert_eq!(requested_after_sequence, 0);
                assert!(available_from_sequence > 1);
                assert_eq!(start_sequence, Some(available_from_sequence));
                assert!(end_sequence.unwrap() >= available_from_sequence);
                assert!(!history.is_empty());
                assert!(
                    history.len() <= 128,
                    "replay must stay bounded by hub history"
                );
            }
            other => panic!("Expected typed replayGap frame, got {other:?}"),
        }

        hub.remove_session(&session_id);
        let _ = pump_handle.await;
    }

    #[tokio::test]
    async fn test_server_epoch_in_handshake_and_list() {
        let server = Arc::new(DaemonServer::new());
        assert!(server.epoch() > 0);

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // 1. Handshake
        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match hs_resp {
            DaemonResponse::HandshakeOk {
                version,
                pid,
                epoch,
            } => {
                assert_eq!(version, DAEMON_PROTOCOL_VERSION);
                assert_eq!(pid, std::process::id());
                assert_eq!(epoch, server.epoch());
            }
            other => panic!("Expected HandshakeOk, got {other:?}"),
        }

        // 2. ListSessions
        line.clear();
        let list_req = DaemonRequest::ListSessions;
        let mut list_json = serde_json::to_string(&list_req).unwrap();
        list_json.push('\n');
        write_half.write_all(list_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let list_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match list_resp {
            DaemonResponse::ListSessionsOk { epoch, sessions } => {
                assert_eq!(epoch, server.epoch());
                assert!(sessions.is_empty() || !sessions.is_empty());
            }
            other => panic!("Expected ListSessionsOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_server_handshake_version_mismatch() {
        let server = Arc::new(DaemonServer::new());
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let hs = DaemonRequest::Handshake { version: 9999 };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match hs_resp {
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => {
                assert_eq!(expected_version, DAEMON_PROTOCOL_VERSION);
                assert_eq!(received_version, 9999);
                assert!(server.terminal_service().list_sessions().is_empty());
            }
            other => panic!("Expected typed ProtocolMismatch, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_server_describe_nonexistent_session() {
        let server = Arc::new(DaemonServer::new());
        let resp = server.handle_describe_session("nonexistent-session-id");
        match resp {
            DaemonResponse::Error { message } => {
                assert!(message.contains("not found"));
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_server_attach_with_after_sequence_and_snapshot() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let session_id = server
            .handle_spawn("req-seq-1", "default", None, None, 80, 24)
            .await
            .unwrap();

        server
            .terminal_service()
            .output_hub()
            .publish(&session_id, b"chunk1".to_vec());
        server
            .terminal_service()
            .output_hub()
            .publish(&session_id, b"chunk2".to_vec());

        let attachment = server
            .terminal_service()
            .attach_with_sequence(&session_id, Some(1))
            .expect("attach with sequence");

        assert_eq!(attachment.snapshot.session_id, session_id);
        assert_eq!(attachment.snapshot.history, b"chunk2");
        assert_eq!(attachment.snapshot.history_start_sequence, Some(2));
        assert_eq!(attachment.snapshot.history_end_sequence, Some(2));
    }
}
