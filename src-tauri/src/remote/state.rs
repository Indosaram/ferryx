use crate::remote::auth::{write_private_json, AuthManager};
use crate::remote::protocol::{RemoteActiveDesktopSelection, RemoteEventMessage};
use crate::terminal::TerminalService;
use crate::worktree::WorkspaceRegistry;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(test)]
use tokio::sync::Notify;
use tokio::sync::{broadcast, watch};

pub type DesktopEventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;
pub const REMOTE_ACTIVE_SELECTION_CHANGED_EVENT: &str = "remote_active_selection_changed";
#[cfg(not(test))]
pub const REMOTE_GATEWAY_PORT: u16 = 43821;
#[cfg(test)]
pub const REMOTE_GATEWAY_PORT: u16 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteNetworkMode {
    Off,
    LocalNetwork,
    Tailscale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RemoteRestartPolicy {
    SessionOnly,
    #[default]
    RestoreListener,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayConfig {
    pub mode: RemoteNetworkMode,
    pub port: u16,
    pub allow_control: bool,
}

impl Default for RemoteGatewayConfig {
    fn default() -> Self {
        Self {
            mode: RemoteNetworkMode::Off,
            port: REMOTE_GATEWAY_PORT,
            allow_control: true,
        }
    }
}

impl RemoteGatewayConfig {
    pub fn restart_policy(&self) -> RemoteRestartPolicy {
        RemoteRestartPolicy::RestoreListener
    }

    fn persisted_snapshot(&self) -> PersistedRemoteGatewayConfig {
        PersistedRemoteGatewayConfig {
            mode: self.mode,
            port: self.port,
            allow_control: self.allow_control,
            restart_policy: self.restart_policy(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRemoteGatewayConfig {
    mode: RemoteNetworkMode,
    port: u16,
    allow_control: bool,
    #[serde(default)]
    restart_policy: RemoteRestartPolicy,
}

#[derive(Debug, Clone)]
struct WorkspaceCacheEntry {
    snapshot: Arc<crate::remote::server::WorkspaceSnapshotCache>,
    revision: u64,
    created_at: std::time::Instant,
}

pub(crate) const WORKSPACE_SNAPSHOT_REFRESH_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(2);

pub struct RemoteGatewayState {
    pub config: RwLock<RemoteGatewayConfig>,
    pub auth_manager: Arc<AuthManager>,
    pub terminal_service: Arc<TerminalService>,
    pub workspace_registry: WorkspaceRegistry,
    pub active_selection: RwLock<Option<RemoteActiveDesktopSelection>>,
    pub active_session_tx: watch::Sender<Option<String>>,
    pub event_tx: broadcast::Sender<String>,
    pub is_running: RwLock<bool>,
    pub bound_address: RwLock<Option<String>>,
    config_path: Option<PathBuf>,
    pub desktop_event_sink: RwLock<Option<DesktopEventSink>>,
    snapshot_cache: RwLock<Option<WorkspaceCacheEntry>>,
    snapshot_lock: tokio::sync::Mutex<()>,
    snapshot_refreshing: AtomicBool,
    #[cfg(test)]
    snapshot_build_count: Arc<AtomicU64>,
    #[cfg(test)]
    snapshot_build_completed: Notify,
    #[cfg(test)]
    snapshot_post_build_hook: Arc<RwLock<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

impl RemoteGatewayState {
    pub fn new(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        Self::new_with_paths(terminal_service, workspace_registry, None, None)
    }

    pub fn new_persistent(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
    ) -> Self {
        let base = remote_data_dir();
        if base.is_none() {
            tracing::warn!(
                "no per-user data directory could be resolved; remote pairing state will be \
                 kept in memory only and lost on exit"
            );
        }
        Self::new_with_paths(
            terminal_service,
            workspace_registry,
            base.as_ref().map(|base| base.join("remote-config.json")),
            base.as_ref().map(|base| base.join("remote-auth.json")),
        )
    }

    pub fn new_with_paths(
        terminal_service: Arc<TerminalService>,
        workspace_registry: WorkspaceRegistry,
        config_path: Option<PathBuf>,
        auth_path: Option<PathBuf>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let (active_session_tx, _) = watch::channel(None);
        let config = config_path
            .as_deref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<PersistedRemoteGatewayConfig>(&bytes).ok())
            .map(|persisted| RemoteGatewayConfig {
                mode: persisted.mode,
                // Port is fixed; ignore persisted value so stale custom ports heal on load.
                port: REMOTE_GATEWAY_PORT,
                allow_control: persisted.allow_control,
            })
            .unwrap_or_default();
        Self {
            config: RwLock::new(config),
            auth_manager: Arc::new(AuthManager::with_persistence(auth_path)),
            terminal_service,
            workspace_registry,
            active_selection: RwLock::new(None),
            active_session_tx,
            event_tx,
            is_running: RwLock::new(false),
            bound_address: RwLock::new(None),
            config_path,
            desktop_event_sink: RwLock::new(None),
            snapshot_cache: RwLock::new(None),
            snapshot_lock: tokio::sync::Mutex::new(()),
            snapshot_refreshing: AtomicBool::new(false),
            #[cfg(test)]
            snapshot_build_count: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            snapshot_build_completed: Notify::new(),
            #[cfg(test)]
            snapshot_post_build_hook: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(test)]
    pub fn snapshot_build_count(&self) -> u64 {
        self.snapshot_build_count.load(Ordering::Acquire)
    }

    pub fn invalidate_workspace_snapshot(&self) {
        *self.snapshot_cache.write() = None;
    }

    pub(crate) async fn workspace_snapshot(
        self: &Arc<Self>,
    ) -> Result<Arc<crate::remote::server::WorkspaceSnapshotCache>, String> {
        self.workspace_snapshot_at(std::time::Instant::now()).await
    }

    pub(crate) async fn workspace_snapshot_at(
        self: &Arc<Self>,
        now: std::time::Instant,
    ) -> Result<Arc<crate::remote::server::WorkspaceSnapshotCache>, String> {
        let current_rev = self.workspace_registry.revision();
        let mut observed_snapshot = None;

        {
            let cache = self.snapshot_cache.read();
            if let Some(entry) = cache.as_ref() {
                observed_snapshot = Some(Arc::clone(&entry.snapshot));
                if entry.revision == current_rev {
                    let snapshot = Arc::clone(&entry.snapshot);
                    if now.saturating_duration_since(entry.created_at)
                        >= WORKSPACE_SNAPSHOT_REFRESH_INTERVAL
                        && self
                            .snapshot_refreshing
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                    {
                        let state = Arc::clone(self);
                        let observed_snapshot = Some(Arc::clone(&snapshot));
                        tokio::spawn(async move {
                            if let Err(error) = state
                                .rebuild_workspace_snapshot(now, observed_snapshot)
                                .await
                            {
                                tracing::warn!(%error, "background workspace snapshot refresh failed");
                            }
                            state.snapshot_refreshing.store(false, Ordering::Release);
                            #[cfg(test)]
                            state.snapshot_build_completed.notify_one();
                        });
                    }
                    return Ok(snapshot);
                }
            }
        }

        self.rebuild_workspace_snapshot(now, observed_snapshot)
            .await
    }

    async fn rebuild_workspace_snapshot(
        &self,
        now: std::time::Instant,
        observed_snapshot: Option<Arc<crate::remote::server::WorkspaceSnapshotCache>>,
    ) -> Result<Arc<crate::remote::server::WorkspaceSnapshotCache>, String> {
        let _guard = self.snapshot_lock.lock().await;
        let current_rev = self.workspace_registry.revision();

        {
            let cache = self.snapshot_cache.read();
            if let Some(entry) = cache.as_ref() {
                if entry.revision == current_rev {
                    match observed_snapshot.as_ref() {
                        None => return Ok(Arc::clone(&entry.snapshot)),
                        Some(observed) if !Arc::ptr_eq(observed, &entry.snapshot) => {
                            return Ok(Arc::clone(&entry.snapshot));
                        }
                        Some(_) => {}
                    }
                }
            }
        }

        let registry = self.workspace_registry.clone();
        #[cfg(test)]
        let build_counter = Arc::clone(&self.snapshot_build_count);
        #[cfg(test)]
        let post_build_hook = Arc::clone(&self.snapshot_post_build_hook);
        let snapshot = tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            build_counter.fetch_add(1, Ordering::AcqRel);
            let snapshot = crate::remote::server::WorkspaceSnapshotCache::build(&registry);
            #[cfg(test)]
            if let Some(hook) = post_build_hook.read().as_ref() {
                hook();
            }
            snapshot
        })
        .await
        .map_err(|error| format!("workspace snapshot task failed: {error}"))?;

        let snapshot_arc = Arc::new(snapshot);
        {
            let mut cache = self.snapshot_cache.write();
            *cache = Some(WorkspaceCacheEntry {
                snapshot: Arc::clone(&snapshot_arc),
                // Use the revision observed before discovery started. A managed
                // mutation that overlaps the blocking Git scan is therefore a
                // mismatch on the next request, never falsely marked as part of
                // this snapshot.
                revision: current_rev,
                created_at: now,
            });
        }
        Ok(snapshot_arc)
    }

    #[cfg(test)]
    pub(crate) fn next_snapshot_build(&self) -> impl std::future::Future<Output = ()> + '_ {
        self.snapshot_build_completed.notified()
    }

    #[cfg(test)]
    pub(crate) fn set_snapshot_post_build_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.snapshot_post_build_hook.write() = hook;
    }

    pub fn set_active_selection(&self, selection: RemoteActiveDesktopSelection) {
        let session_id = selection.session_id.clone();
        let payload = serde_json::to_value(&selection).unwrap_or(serde_json::Value::Null);
        *self.active_selection.write() = Some(selection);
        // `send` fails and discards the value when no receiver is alive, which is the normal
        // state before any remote client attaches. `send_replace` stores it regardless so a
        // later subscriber observes the current selection instead of the initial `None`.
        self.active_session_tx.send_replace(session_id);
        self.emit_active_selection_changed(payload);
    }

    pub fn clear_active_selection(&self) {
        *self.active_selection.write() = None;
        self.active_session_tx.send_replace(None);
        self.emit_active_selection_changed(serde_json::Value::Null);
    }

    pub fn set_active_selection_opt(&self, selection: Option<RemoteActiveDesktopSelection>) {
        match selection {
            Some(sel) => self.set_active_selection(sel),
            None => self.clear_active_selection(),
        }
    }

    pub fn active_session_watch_rx(&self) -> watch::Receiver<Option<String>> {
        self.active_session_tx.subscribe()
    }

    pub fn active_selection(&self) -> Option<RemoteActiveDesktopSelection> {
        self.active_selection.read().clone()
    }

    pub fn set_desktop_event_sink(&self, sink: DesktopEventSink) {
        *self.desktop_event_sink.write() = Some(sink);
    }

    pub fn emit_desktop_event(&self, event: &str, payload: serde_json::Value) {
        if let Some(sink) = self.desktop_event_sink.read().as_ref() {
            sink(event, payload.clone());
        }
        let _ = self.event_tx.send(
            serde_json::to_string(&RemoteEventMessage {
                event: event.to_string(),
                payload,
            })
            .unwrap_or_default(),
        );
    }

    pub fn emit_event(&self, event_json: String) {
        let _ = self.event_tx.send(event_json);
    }

    fn emit_active_selection_changed(&self, payload: serde_json::Value) {
        if let Ok(event) = serde_json::to_string(&RemoteEventMessage {
            event: REMOTE_ACTIVE_SELECTION_CHANGED_EVENT.to_string(),
            payload,
        }) {
            let _ = self.event_tx.send(event);
        }
    }

    pub fn persist_config(&self) -> std::io::Result<()> {
        let Some(path) = self.config_path.as_deref() else {
            return Ok(());
        };
        write_private_json(path, &self.config.read().persisted_snapshot())
    }
}

/// Per-user directory sources consulted in order, as `(environment variable, subdirectory)`.
///
/// Windows does not define `HOME`; per-user application state belongs under `LOCALAPPDATA`,
/// whose default ACL excludes other standard users.
#[cfg(windows)]
const DATA_DIR_SOURCES: &[(&str, &str)] = &[("LOCALAPPDATA", "Ferryx"), ("USERPROFILE", ".ferryx")];
#[cfg(not(windows))]
const DATA_DIR_SOURCES: &[(&str, &str)] = &[("HOME", ".ferryx")];

/// Resolves the remote data directory from an arbitrary environment lookup.
///
/// Returns `None` when no per-user location can be determined. Callers then run without
/// persistence: this file holds plaintext device tokens, so falling back to a shared
/// scratch directory such as [`std::env::temp_dir`] would place credentials somewhere
/// other accounts may read and automated cleaners routinely purge.
fn resolve_remote_data_dir<F>(lookup: F) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    if let Some(path) = lookup("FERRYX_DATA_DIR") {
        return Some(PathBuf::from(path).join("remote"));
    }
    DATA_DIR_SOURCES
        .iter()
        .find_map(|(variable, subdirectory)| {
            lookup(variable).map(|base| PathBuf::from(base).join(subdirectory).join("remote"))
        })
}

fn remote_data_dir() -> Option<PathBuf> {
    resolve_remote_data_dir(|variable| std::env::var_os(variable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::DevicePermission;
    use crate::terminal::{PtyManager, TerminalOutputHub};

    fn test_state(
        config_path: PathBuf,
        auth_path: PathBuf,
    ) -> (RemoteGatewayState, WorkspaceRegistry, Arc<TerminalService>) {
        let pty = Arc::new(PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal = Arc::new(TerminalService::new(pty, hub));
        let registry = WorkspaceRegistry::new();
        let state = RemoteGatewayState::new_with_paths(
            Arc::clone(&terminal),
            registry.clone(),
            Some(config_path),
            Some(auth_path),
        );
        (state, registry, terminal)
    }

    #[test]
    fn enabled_gateway_persists_config_and_restores_on_reopen() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config_path = dir.path().join("config.json");
        let auth_path = dir.path().join("auth.json");
        let (state, registry, terminal) = test_state(config_path.clone(), auth_path.clone());

        {
            let mut config = state.config.write();
            config.mode = RemoteNetworkMode::LocalNetwork;
            config.port = 45678;
            config.allow_control = false;
        }
        *state.is_running.write() = true;
        *state.bound_address.write() = Some("0.0.0.0:45678".into());
        let code = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, device) = state
            .auth_manager
            .exchange_pairing_code(&code, "Phone")
            .expect("pair");
        state.persist_config().expect("persist");

        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&config_path).expect("read persisted config"))
                .expect("parse persisted config");
        assert_eq!(on_disk["mode"], "localNetwork");
        assert_eq!(on_disk["port"], 45678);
        assert_eq!(on_disk["allowControl"], false);
        assert_eq!(on_disk["restartPolicy"], "restoreListener");

        let reopened = RemoteGatewayState::new_with_paths(
            terminal,
            registry,
            Some(config_path),
            Some(auth_path),
        );
        let config = reopened.config.read().clone();
        assert_eq!(config.mode, RemoteNetworkMode::LocalNetwork);
        assert_eq!(config.port, REMOTE_GATEWAY_PORT);
        assert!(!config.allow_control);
        assert_eq!(
            config.restart_policy(),
            RemoteRestartPolicy::RestoreListener
        );
        assert!(!*reopened.is_running.read());
        assert!(reopened.bound_address.read().is_none());

        let devices = reopened.auth_manager.list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, device.id);
        assert!(reopened.auth_manager.validate_token(&token).is_ok());
    }

    #[test]
    fn daemon_default_state_is_off_and_uses_supplied_terminal_service() {
        let pty = Arc::new(PtyManager::new());
        let hub = Arc::new(TerminalOutputHub::default());
        let terminal = Arc::new(TerminalService::new(pty, hub));
        let state = RemoteGatewayState::new(Arc::clone(&terminal), WorkspaceRegistry::new());

        assert_eq!(state.config.read().mode, RemoteNetworkMode::Off);
        assert!(!*state.is_running.read());
        assert!(Arc::ptr_eq(&state.terminal_service, &terminal));
    }

    #[test]
    fn persisted_enabled_config_file_loads_enabled_mode_with_unbound_initial_state() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let config_path = dir.path().join("config.json");
        let auth_path = dir.path().join("auth.json");
        std::fs::write(
            &config_path,
            r#"{"mode":"localNetwork","port":41234,"allowControl":true}"#,
        )
        .expect("write enabled config");

        let (state, _, _) = test_state(config_path, auth_path);
        let config = state.config.read().clone();
        assert_eq!(config.mode, RemoteNetworkMode::LocalNetwork);
        assert_eq!(config.port, REMOTE_GATEWAY_PORT);
        assert!(config.allow_control);
        assert_eq!(
            config.restart_policy(),
            RemoteRestartPolicy::RestoreListener
        );
        assert!(!*state.is_running.read());
        assert!(state.bound_address.read().is_none());
    }

    /// Builds a lookup over an explicit variable set so the platform-specific resolution order
    /// can be exercised on any host without mutating process environment.
    fn lookup_from<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        |variable: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == variable)
                .map(|(_, value)| OsString::from(*value))
        }
    }

    #[test]
    fn explicit_data_dir_override_wins_over_platform_home() {
        let resolved = resolve_remote_data_dir(lookup_from(&[
            ("FERRYX_DATA_DIR", "/explicit/base"),
            ("HOME", "/home/user"),
            ("LOCALAPPDATA", r"C:\Users\user\AppData\Local"),
        ]))
        .expect("override must resolve");

        assert_eq!(resolved, PathBuf::from("/explicit/base").join("remote"));
    }

    #[test]
    fn credentials_are_never_placed_in_a_shared_scratch_directory() {
        // Windows leaves HOME unset, which previously fell through to `std::env::temp_dir()` and
        // wrote plaintext device tokens into the shared TEMP directory.
        assert!(
            resolve_remote_data_dir(lookup_from(&[("TMP", r"C:\Windows\Temp")])).is_none(),
            "with no per-user variable set, persistence must be declined rather than redirected"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefers_localappdata_and_accepts_userprofile() {
        let localappdata = resolve_remote_data_dir(lookup_from(&[
            ("LOCALAPPDATA", r"C:\Users\user\AppData\Local"),
            ("USERPROFILE", r"C:\Users\user"),
        ]))
        .expect("LOCALAPPDATA must resolve");
        assert_eq!(
            localappdata,
            PathBuf::from(r"C:\Users\user\AppData\Local")
                .join("Ferryx")
                .join("remote")
        );

        let userprofile =
            resolve_remote_data_dir(lookup_from(&[("USERPROFILE", r"C:\Users\user")]))
                .expect("USERPROFILE must resolve when LOCALAPPDATA is absent");
        assert_eq!(
            userprofile,
            PathBuf::from(r"C:\Users\user")
                .join(".ferryx")
                .join("remote")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_resolves_under_home_and_ignores_windows_variables() {
        let resolved = resolve_remote_data_dir(lookup_from(&[("HOME", "/home/user")]))
            .expect("HOME must resolve");
        assert_eq!(
            resolved,
            PathBuf::from("/home/user").join(".ferryx").join("remote")
        );

        assert!(
            resolve_remote_data_dir(lookup_from(&[(
                "LOCALAPPDATA",
                r"C:\Users\user\AppData\Local"
            )]))
            .is_none(),
            "Windows-only variables must not be honored on Unix"
        );
    }
}
