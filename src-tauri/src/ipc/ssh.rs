use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::ssh::config::parse_ssh_config;
use crate::ssh::SshHost;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Manager, Runtime};

/// Serializes every read-modify-write of the SSH host store.
///
/// Each mutating command runs its load/modify/save on its own `run_blocking`
/// thread, so two concurrent commands would otherwise both read the same
/// snapshot and the second `save_store` would silently discard the first
/// command's change. The store is a single process-wide file, so one global
/// lock is the whole fix.
static SSH_STORE_LOCK: Mutex<()> = Mutex::new(());
// Credential mutations and probes must not race a replacement password in the
// other process. This lock spans the GUI/daemon transaction, not network I/O on
// the credential store's synchronous mutex.
static SSH_CREDENTIAL_TRANSACTION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static SSH_TAILSCALE_SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Acquires the store lock, recovering from a poisoned mutex.
///
/// A panic in one command must not permanently disable SSH host editing: the
/// guarded data lives on disk, not in the mutex, so the poison flag carries no
/// corrupted in-memory state to protect.
fn lock_ssh_store() -> std::sync::MutexGuard<'static, ()> {
    SSH_STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTargetSummary {
    pub host: SshHost,
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub environment: Option<crate::ssh::runtime::RemoteEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub helper: Option<crate::ssh::helper_setup::HelperProbeState>,
    pub diagnostic: Option<IpcError>,
    pub checked_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTailscaleSyncResult {
    pub hosts: Vec<SshHost>,
    pub targets: Vec<SshTargetSummary>,
    pub discovery_error: Option<IpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSshConfigResult {
    pub path: String,
    pub exists: bool,
    pub raw_text: String,
    pub hosts: Vec<SshHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshHostStore {
    #[serde(default, deserialize_with = "deserialize_resilient_hosts")]
    pub hosts: Vec<SshHost>,
    #[serde(default)]
    pub tombstones: Vec<String>,
}

#[cfg(test)]
mod tailscale_sync_tests {
    use super::*;
    use crate::remote::discovery::tailscale::TailscaleSshPeer;

    fn peer(id: &str, online: bool) -> TailscaleSshPeer {
        TailscaleSshPeer { node_id: id.into(), hostname: id.into(), dns_name: Some(format!("{id}.tailnet.ts.net")), ipv4: format!("100.64.0.{}", id.len()), online, os: Some("linux".into()) }
    }

    fn host(id: &str, name: &str, source: crate::ssh::SshHostSource) -> SshHost {
        SshHost { id: id.into(), label: name.into(), hostname: name.into(), username: Some("alice".into()), port: Some(2222), identity_file: Some("~/.ssh/id_test".into()), jump_host: Some("bastion".into()), source, auth_method: crate::ssh::SshAuthMethod::Key, disabled: None, shell: None }
    }

    #[test]
    fn merge_preserves_saved_identity_credentials_and_stable_node_updates() {
        let p = peer("node-a", true);
        let alias = host("cfg-box", &p.hostname, crate::ssh::SshHostSource::Config);
        let mut store = SshHostStore { hosts: vec![alias.clone()], tombstones: Vec::new() };
        merge_tailscale_peers(&mut store, std::slice::from_ref(&p), std::slice::from_ref(&alias));
        assert_eq!(store.hosts, vec![alias.clone()]);
        let mut deleted = SshHostStore { hosts: Vec::new(), tombstones: vec![format!("tailscale-host:{}", p.hostname)] };
        merge_tailscale_peers(&mut deleted, std::slice::from_ref(&p), std::slice::from_ref(&alias));
        assert!(deleted.hosts.is_empty());
        let mut imported = SshHostStore::default();
        merge_tailscale_peers(&mut imported, std::slice::from_ref(&p), &[alias]);
        assert_eq!(imported.hosts[0].id, "tailscale:node-a");
        assert_eq!(imported.hosts[0].username.as_deref(), Some("alice"));
        imported.hosts[0].label = "Custom label".into();
        let changed = TailscaleSshPeer { ipv4: "100.64.9.9".into(), hostname: "new-name".into(), dns_name: None, ..p.clone() };
        merge_tailscale_peers(&mut imported, &[changed], &[]);
        assert_eq!(imported.hosts.len(), 1);
        assert_eq!(imported.hosts[0].hostname, "100.64.9.9");
        assert_eq!(imported.hosts[0].label, "Custom label");
        imported.hosts.clear();
        imported.tombstones.push("tailscale:node-a".into());
        merge_tailscale_peers(&mut imported, &[p], &[]);
        assert!(imported.hosts.is_empty());
    }

    #[tokio::test]
    async fn production_orchestration_skips_ineligible_hosts_and_processes_entire_queue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts.json");
        let ids = ["disabled-a", "disabled-b", "disabled-c", "disabled-d", "online-a", "online-b", "offline", "mobile"];
        let mut peers: Vec<_> = ids.iter().enumerate().map(|(index, id)| TailscaleSshPeer { ipv4: format!("100.64.0.{}", index + 1), ..peer(id, *id != "offline") }).collect();
        peers.last_mut().unwrap().os = Some("android".into());
        let mut store = SshHostStore::default();
        merge_tailscale_peers(&mut store, &peers, &[]);
        for h in &mut store.hosts { if h.id.contains("disabled") { h.disabled = Some(true); } }
        save_store(&path, &store).unwrap();
        let calls = std::sync::Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let status = crate::remote::discovery::tailscale::TailscaleSshStatus { peers: peers.clone(), self_ipv4: None };
        let result = sync_tailscale_peers_with(path, status, Some(Vec::new()), move |host, _| {
            let calls = recorded.clone();
            async move {
                calls.lock().unwrap().push(host.id.clone());
                Ok(SshTargetSummary { host, reachable: true, last_error: None, environment: None, helper: Some(crate::ssh::helper_setup::HelperProbeState::Installed), diagnostic: None, checked_at: 1 })
            }
        }).await.unwrap();
        let mut called = calls.lock().unwrap().clone(); called.sort();
        assert_eq!(called, vec!["tailscale:online-a", "tailscale:online-b"]);
        // The offline peer's row is pruned, so it no longer has a host row to report against:
        // 7 rows survive (4 disabled + 2 online + the unsupported mobile peer) and the target
        // list is one unreachable mobile peer plus the two online provisions.
        assert_eq!(result.hosts.len(), 7);
        assert_eq!(result.targets.len(), 3);
        assert!(result.targets.iter().filter(|t| !t.reachable).all(|t| t.diagnostic.is_some()));
    }

    #[test]
    fn pruning_drops_vanished_peers_but_keeps_manual_disabled_and_self_rows() {
        let live = peer("live", true);
        let gone = peer("gone", false);
        let mut store = SshHostStore {
            hosts: vec![
                host("tailscale:live", &live.hostname, crate::ssh::SshHostSource::Tailscale),
                host("tailscale:gone", &gone.hostname, crate::ssh::SshHostSource::Tailscale),
                host("manual-box", "manual-box", crate::ssh::SshHostSource::Manual),
                host("ssh-stale-alias", "stale-alias", crate::ssh::SshHostSource::Config),
                host("self-address", "100.78.73.127", crate::ssh::SshHostSource::Tailscale),
            ],
            tombstones: Vec::new(),
        };
        // "omarchy2" is stored under a tailscale id because discovery adopted the alias and login
        // the user wrote in ~/.ssh/config. The box is not in the tailnet right now, but the user
        // still owns the entry, so pruning must keep it while dropping the row nobody owns.
        let mut config_owned = host("tailscale:omarchy2", "100.106.164.55", crate::ssh::SshHostSource::Tailscale);
        config_owned.label = "omarchy2".into();
        let mut discovered_junk = host("tailscale:codex-2", "100.102.43.7", crate::ssh::SshHostSource::Tailscale);
        discovered_junk.label = "codex-environment-2".into();
        store.hosts.push(config_owned);
        store.hosts.push(discovered_junk);
        let mut config_entry = host("ssh-omarchy2", "100.106.164.55", crate::ssh::SshHostSource::Config);
        config_entry.label = "omarchy2".into();
        retain_vanished_hosts(&mut store, &[live, gone], Some("100.78.73.127"), Some(&[config_entry]));
        let ids: Vec<&str> = store.hosts.iter().map(|host| host.id.as_str()).collect();
        assert_eq!(ids, vec!["tailscale:live", "manual-box", "self-address", "tailscale:omarchy2"]);

        // An unreadable config must not be treated as an empty one.
        let mut store = SshHostStore {
            hosts: vec![host("ssh-kept", "kept", crate::ssh::SshHostSource::Config)],
            tombstones: Vec::new(),
        };
        retain_vanished_hosts(&mut store, &[], None, None);
        assert_eq!(store.hosts.len(), 1);

        // ...and the same holds for a discovered row: an unreadable config keeps it, because the
        // sync cannot prove the user does not own it.
        let mut store = SshHostStore {
            hosts: vec![host("tailscale:maybe-ours", "100.102.43.7", crate::ssh::SshHostSource::Tailscale)],
            tombstones: Vec::new(),
        };
        retain_vanished_hosts(&mut store, &[], None, None);
        assert_eq!(store.hosts.len(), 1, "an unreadable config must not prune discovered rows either");

        // Disabling a row is an explicit decision to keep it out of use, not to forget it.
        let mut disabled = host("tailscale:sleeping", "sleeping", crate::ssh::SshHostSource::Tailscale);
        disabled.disabled = Some(true);
        let mut store = SshHostStore { hosts: vec![disabled], tombstones: Vec::new() };
        retain_vanished_hosts(&mut store, &[], None, Some(&[]));
        assert_eq!(store.hosts.len(), 1);
    }

    #[test]
    fn dedupe_drops_only_discovered_rows_shadowed_by_a_user_row() {
        let manual = host("mine", "100.65.239.35", crate::ssh::SshHostSource::Manual);
        let imported = host("ssh-maho-mac", "100.65.239.35", crate::ssh::SshHostSource::Config);
        let discovered = host("tailscale:node-mac", "100.65.239.35", crate::ssh::SshHostSource::Tailscale);
        let mut store = SshHostStore { hosts: vec![discovered, imported.clone(), manual.clone()], tombstones: Vec::new() };
        dedupe_tailscale_rows(&mut store);
        assert_eq!(store.hosts, vec![imported, manual], "the discovered row goes; both user rows stay");

        // A discovered row shadowed by a config alias is redundant.
        let imported = host("ssh-maho-mac", "100.65.239.35", crate::ssh::SshHostSource::Config);
        let discovered = host("tailscale:node-mac", "100.65.239.35", crate::ssh::SshHostSource::Tailscale);
        let mut store = SshHostStore { hosts: vec![discovered, imported.clone()], tombstones: Vec::new() };
        dedupe_tailscale_rows(&mut store);
        assert_eq!(store.hosts, vec![imported]);

        // A discovered row nothing shadows is the only row for that machine, so it stays.
        let lonely = host("tailscale:node-lonely", "100.99.99.99", crate::ssh::SshHostSource::Tailscale);
        let mut store = SshHostStore { hosts: vec![lonely.clone()], tombstones: Vec::new() };
        dedupe_tailscale_rows(&mut store);
        assert_eq!(store.hosts, vec![lonely]);

        // Same address, two logins: distinct machines to the user, so both stay.
        let mut root = host("ssh-maho-mac-root", "100.65.239.35", crate::ssh::SshHostSource::Config);
        root.username = Some("root".into());
        let mut store = SshHostStore { hosts: vec![host("ssh-maho-mac", "100.65.239.35", crate::ssh::SshHostSource::Config), root.clone()], tombstones: Vec::new() };
        dedupe_tailscale_rows(&mut store);
        assert_eq!(store.hosts.len(), 2);
        assert!(store.hosts.contains(&root));
    }

    #[test]
    fn dedupe_never_folds_two_config_aliases_of_the_same_machine() {
        // A real ~/.ssh/config carries several aliases for one box - maho-win, maho-scalper and
        // maho-steam all resolve to the same address and login. They are names the user wrote on
        // purpose, so discovery must not delete two of them as duplicates.
        let mut scalper = host("ssh-maho-scalper", "100.68.224.49", crate::ssh::SshHostSource::Config);
        scalper.username = Some("sook".into());
        let mut steam = host("ssh-maho-steam", "100.68.224.49", crate::ssh::SshHostSource::Config);
        steam.username = Some("sook".into());
        let mut primary = host("ssh-maho-win", "100.68.224.49", crate::ssh::SshHostSource::Config);
        primary.username = Some("sook".into());
        // Discovery reports no login for a peer, so the discovered row has no username to match
        // the aliases' login. That mismatch is exactly why a discovered row must not be folded
        // into an alias: they are not provably the same entry.
        let discovered = host("tailscale:node-win", "100.68.224.49", crate::ssh::SshHostSource::Tailscale);
        let mut store = SshHostStore { hosts: vec![primary.clone(), scalper.clone(), steam.clone(), discovered.clone()], tombstones: Vec::new() };
        dedupe_tailscale_rows(&mut store);
        assert_eq!(store.hosts.len(), 4, "all three config aliases survive and the discovered row is not folded into them");
        assert!(store.hosts.contains(&primary) && store.hosts.contains(&scalper) && store.hosts.contains(&steam));
        assert!(store.hosts.contains(&discovered));
    }

    #[tokio::test]
    async fn target_reports_authentication_and_provisioning_failures() {
        let env = crate::ssh::runtime::RemoteEnvironment { platform: crate::ssh::runtime::RemotePlatform::Posix, executor: crate::ssh::runtime::RemoteExecutor::Sh, version: "Linux".into(), home: "/home/test".into(), temp: "/tmp".into(), git: true };
        let h = host("test", "host", crate::ssh::SshHostSource::Tailscale);
        let failed = sync_tailscale_target_with(h.clone(), || async { Err(IpcError::new(IpcErrorCode::IoError, "authentication failed")) }, |_| async { panic!("must not provision after failed detection") }).await.unwrap();
        assert!(!failed.reachable);
        assert!(failed.diagnostic.is_some());
        let failed = sync_tailscale_target_with(h.clone(), || async { Ok(env.clone()) }, |_| async { Err(IpcError::new(IpcErrorCode::IoError, "helper startup failed")) }).await.unwrap();
        assert!(failed.reachable);
        assert_eq!(failed.helper, Some(crate::ssh::helper_setup::HelperProbeState::Unknown));
        assert!(failed.diagnostic.is_some());
        let ready = sync_tailscale_target_with(h, || async { Ok(env.clone()) }, |_| async { Ok(()) }).await.unwrap();
        assert_eq!(ready.helper, Some(crate::ssh::helper_setup::HelperProbeState::Installed));
        assert!(ready.diagnostic.is_none());
    }
}

pub fn deserialize_resilient_hosts<'de, D>(deserializer: D) -> Result<Vec<SshHost>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Vec<serde_json::Value>> = Option::deserialize(deserializer)?;
    let Some(values) = opt else {
        return Ok(Vec::new());
    };
    let mut hosts: Vec<SshHost> = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        let id_hint = value
            .get("id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        match serde_json::from_value::<SshHost>(value) {
            Ok(host) => {
                if let Some(position) = hosts.iter().position(|existing| existing.id == host.id) {
                    tracing::warn!(
                        "Replacing duplicate SSH host id '{}' at index {index}; keeping the latest configuration",
                        host.id
                    );
                    hosts[position] = host;
                } else {
                    hosts.push(host);
                }
            }
            Err(e) => {
                let id_display = id_hint.as_deref().unwrap_or("<unknown>");
                tracing::warn!(
                    "Skipping unparseable SSH host at index {index} (id: {id_display}): {e}"
                );
            }
        }
    }
    Ok(hosts)
}

#[cfg(feature = "desktop")]
pub(crate) fn get_ssh_store_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR") {
        return Ok(PathBuf::from(dir).join("ssh_hosts.json"));
    }
    let app_dir = app.path().app_data_dir().map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to resolve app data dir: {}", e),
        )
    })?;
    if crate::daemon::server::is_dev_runtime() {
        Ok(app_dir.join("dev").join("ssh_hosts.json"))
    } else {
        Ok(app_dir.join("ssh_hosts.json"))
    }
}

#[cfg(feature = "desktop")]
pub fn resolve_home_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().home_dir().ok().or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    })
}

#[cfg(feature = "desktop")]
pub fn system_ssh_config_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    resolve_home_dir(app).map(|home| home.join(".ssh").join("config"))
}

pub(crate) fn expand_tilde_path(raw: &str, home: Option<&Path>) -> PathBuf {
    if raw == "~" {
        home.map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(raw))
    } else if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        match home {
            Some(h) => {
                let mut path = h.to_path_buf();
                for component in stripped.split(['/', '\\']) {
                    if !component.is_empty() {
                        path.push(component);
                    }
                }
                path
            }
            None => PathBuf::from(raw),
        }
    } else {
        PathBuf::from(raw)
    }
}

fn read_ssh_config_file(path: &Path) -> Result<SystemSshConfigResult, IpcError> {
    let path_str = path.to_string_lossy().into_owned();
    if !path.is_file() {
        return Ok(SystemSshConfigResult {
            path: path_str,
            exists: false,
            raw_text: String::new(),
            hosts: Vec::new(),
        });
    }

    let raw_text = std::fs::read_to_string(path).map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to read SSH config at {}: {}", path_str, e),
        )
    })?;
    let parsed = parse_ssh_config(&raw_text);
    let hosts = crate::ssh::config::import_aliases(&parsed, &[]);

    Ok(SystemSshConfigResult {
        path: path_str,
        exists: true,
        raw_text,
        hosts,
    })
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_read_system_config<R: Runtime>(
    app: AppHandle<R>,
    config_path: Option<String>,
) -> Result<SystemSshConfigResult, IpcError> {
    run_blocking(move || {
        let home = resolve_home_dir(&app);
        let selected = config_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|raw| expand_tilde_path(raw, home.as_deref()));

        let Some(path) = selected.or_else(|| system_ssh_config_path(&app)) else {
            return Ok(SystemSshConfigResult {
                path: String::new(),
                exists: false,
                raw_text: String::new(),
                hosts: Vec::new(),
            });
        };

        read_ssh_config_file(&path)
    })
    .await
}

fn load_store(path: &PathBuf) -> Result<SshHostStore, IpcError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Failed to parse ssh store: {}", e),
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SshHostStore::default()),
        Err(e) => Err(IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to read ssh store: {}", e),
        )),
    }
}

fn save_store(path: &PathBuf, store: &SshHostStore) -> Result<(), IpcError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to create ssh store dir: {}", e),
            )
        })?;
    }
    let serialized = serde_json::to_string_pretty(store)
        .map_err(|e| IpcError::internal(format!("Failed to serialize ssh store: {}", e)))?;
    // A per-write unique temp name: two concurrent writers sharing one fixed
    // `.json.tmp` would interleave their bytes and rename a corrupt blend into
    // place. The lock above already serializes in-process callers; this keeps a
    // second process (CLI, a stale instance) from colliding too.
    let tmp = path.with_extension(format!(
        "json.tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let write_result =
        std::fs::write(&tmp, serialized.as_bytes()).and_then(|()| std::fs::rename(&tmp, path));
    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write_result.map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to write ssh store: {}", e),
        )
    })?;
    crate::ssh::projects::invalidate_store_memo(path);
    Ok(())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_list_hosts<R: Runtime>(app: AppHandle<R>) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || Ok(load_store(&path)?.hosts)).await
}

/// One resource sample from an SSH host named by its store id.
///
/// The host id is resolved from the daemon-owned store rather than trusted from the caller, so
/// this reads a machine the user actually configured and never an arbitrary address.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_host_resources<R: Runtime>(
    app: AppHandle<R>,
    host_id: String,
) -> Result<crate::daemon::resource_usage::HostResourceSnapshot, IpcError> {
    let path = get_ssh_store_path(&app)?;
    let host = run_blocking(move || crate::ssh::projects::enabled_host(&path, &host_id)).await?;
    let environment = crate::ssh::runtime::detect(&host).await?;
    crate::ssh::resources::sample(&host, &environment).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_import_config<R: Runtime>(
    app: AppHandle<R>,
    config_text: String,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || import_config_into_store(&path, &config_text)).await
}

fn import_config_into_store(path: &PathBuf, config_text: &str) -> Result<Vec<SshHost>, IpcError> {
    let _guard = lock_ssh_store();
    let mut store = load_store(path)?;
    let parsed = parse_ssh_config(config_text);
    let already_stored: Vec<String> = store.hosts.iter().map(|host| host.key()).collect();
    let imported = crate::ssh::config::import_aliases(&parsed, &already_stored);
    for host in imported {
        let key = host.key();
        store.tombstones.retain(|tombstone| tombstone != &key);
        if let Some(position) = store
            .hosts
            .iter()
            .position(|existing| existing.id == host.id)
        {
            store.hosts[position] = host;
        } else {
            store.hosts.push(host);
        }
    }
    save_store(path, &store)?;
    Ok(store.hosts)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_update_host<R: Runtime>(
    app: AppHandle<R>,
    host: SshHost,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || {
        let _guard = lock_ssh_store();
        let mut store = load_store(&path)?;
        if let Some(slot) = store
            .hosts
            .iter_mut()
            .find(|existing| existing.id == host.id)
        {
            *slot = host;
        } else {
            store.hosts.push(host);
        }
        save_store(&path, &store)?;
        Ok(store.hosts)
    })
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_delete_host<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || {
        let _guard = lock_ssh_store();
        let mut store = load_store(&path)?;
        if let Some(position) = store.hosts.iter().position(|host| host.id == id) {
            let removed = store.hosts.remove(position);
            let discovery_key = format!("tailscale-host:{}", removed.hostname.to_ascii_lowercase());
            if !store.tombstones.contains(&discovery_key) {
                store.tombstones.push(discovery_key);
            }
            if removed.id.starts_with("tailscale:") && !store.tombstones.contains(&removed.id) {
                store.tombstones.push(removed.id.clone());
            }
            if removed.source == crate::ssh::SshHostSource::Config {
                let key = removed.key();
                if !store.tombstones.contains(&key) {
                    store.tombstones.push(key);
                }
            }
        }
        save_store(&path, &store)?;
        Ok(store.hosts)
    })
    .await
}

/// Build a live SSH target summary for a host. Shared by the Tauri command and
/// integration QA tests; callers hold the credential transaction when probing
/// mutable credentials. This never touches daemon-stored passwords.
pub async fn ssh_test_connection_summary(host: SshHost) -> SshTargetSummary {
    let result = crate::ssh::runtime::detect(&host).await;
    let reachable = result.is_ok();
    let (environment, diagnostic) = match result {
        Ok(environment) => (Some(environment), None),
        Err(error) => (None, Some(error)),
    };
    // Non-installing helper check, only when the host is reachable. Any probe
    // failure (timeout, transport error) classifies to Unknown, never an Err.
    let helper = match &environment {
        Some(environment) => Some(crate::ssh::helper_setup::probe_ready(&host, environment).await),
        None => None,
    };
    SshTargetSummary {
        host,
        reachable,
        last_error: diagnostic.as_ref().map(|e| e.message.clone()),
        environment,
        helper,
        diagnostic,
        checked_at: now_millis(),
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_test_connection(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    host: SshHost,
    candidate_password: Option<crate::ssh::password::Password>,
) -> Result<SshTargetSummary, IpcError> {
    let _transaction = SSH_CREDENTIAL_TRANSACTION.lock().await;
    if let Some(password) = candidate_password {
        let candidate = crate::ssh::password::push_candidate(&host, password)?;
        let summary = ssh_test_connection_summary(host.clone()).await;
        drop(candidate);
        return Ok(summary);
    }
    let summary = ssh_test_connection_summary(host.clone()).await;
    let authentication_stage_failed = summary
        .diagnostic
        .as_ref()
        .and_then(|e| e.details.as_ref())
        .and_then(|v| v.get("stage"))
        .and_then(|v| v.as_str())
        == Some("authentication");
    if authentication_stage_failed {
        clear_password_in_daemon(&daemon_client, host).await?;
    }
    Ok(summary)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_set_password(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    host: SshHost,
    password: crate::ssh::password::Password,
) -> Result<(), IpcError> {
    let _transaction = SSH_CREDENTIAL_TRANSACTION.lock().await;
    let credential_host = host.clone();
    let credential = password.clone();
    run_blocking(move || crate::ssh::password::set(&credential_host, credential)).await?;
    match daemon_client
        .send_request(crate::daemon::protocol::DaemonRequest::SshPassword {
            host: host.clone(),
            password: Some(password),
        })
        .await
    {
        Ok(crate::daemon::protocol::DaemonResponse::Pong) => Ok(()),
        result => {
            crate::ssh::password::clear(&host)?;
            match result {
                Err(error)
                | Ok(crate::daemon::protocol::DaemonResponse::WorktreeError { error }) => {
                    Err(error)
                }
                _ => Err(IpcError::internal(
                    "Daemon does not support SSH password credentials",
                )),
            }
        }
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_clear_password(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    host: SshHost,
) -> Result<(), IpcError> {
    let _transaction = SSH_CREDENTIAL_TRANSACTION.lock().await;
    clear_password_in_daemon(&daemon_client, host).await
}

async fn clear_password_in_daemon(
    daemon_client: &crate::daemon::client::DaemonClient,
    host: SshHost,
) -> Result<(), IpcError> {
    crate::ssh::password::clear(&host)?;
    match daemon_client
        .send_request(crate::daemon::protocol::DaemonRequest::SshPassword {
            host,
            password: None,
        })
        .await?
    {
        crate::daemon::protocol::DaemonResponse::Pong => Ok(()),
        crate::daemon::protocol::DaemonResponse::WorktreeError { error } => Err(error),
        _ => Err(IpcError::internal(
            "Daemon does not support SSH password credentials",
        )),
    }
}

#[cfg(feature = "desktop")]
/// Explicit user-selected artifact installation; no build or download is performed.
#[tauri::command]
pub async fn cmd_ssh_install_project_helper<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    local_binary: PathBuf,
) -> Result<crate::ssh::helper_setup::HelperLocation, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let (_, host) =
        super::run_blocking(move || crate::ssh::projects::resolve(&store, &workspace_id)).await?;
    let environment = crate::ssh::runtime::detect(&host).await?;
    let location = crate::ssh::helper_setup::qualified_location(
        &host,
        &environment,
        crate::ssh::helper_runtime::process::HELPER_VERSION,
    )?;
    crate::ssh::helper_setup::install(&host, &environment, &location, &local_binary).await?;
    crate::ssh::helper_setup::ensure_started(&host, &environment, &location).await?;
    Ok(location)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_prepare_integration(host: SshHost) -> Result<(), IpcError> {
    crate::ssh::direct::ensure_remote_extension_installed(&host).await
}

#[cfg(feature = "desktop")]
/// Deletes the Ferryx agent-state extension from the stored host `host_id`, file by file, only
/// where it is still byte-for-byte a payload Ferryx wrote.
#[tauri::command]
pub async fn cmd_ssh_remove_integration<R: Runtime>(
    app: AppHandle<R>,
    host_id: String,
) -> Result<crate::ssh::operations::RemoteRemovalReport, IpcError> {
    let path = get_ssh_store_path(&app)?;
    let host = run_blocking(move || {
        load_store(&path)?
            .hosts
            .into_iter()
            .find(|host| host.id == host_id)
            .ok_or_else(|| IpcError::new(IpcErrorCode::InvalidArgument, "Unknown SSH host"))
    })
    .await?;
    let environment = crate::ssh::runtime::detect(&host).await?;
    crate::ssh::operations::remove_integration(&host, &environment).await
}

/// Wire code for every failure after the SSH install of a Ferryx node has started.
pub(crate) const SSH_NODE_INSTALL_FAILED: &str = "SSH_NODE_INSTALL_FAILED";
const NODE_INSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
const NODE_INSTALL_STDERR_TAIL_BYTES: usize = 2000;
/// Remote exit status the install command uses when install.sh could not be downloaded.
const NODE_INSTALL_DOWNLOAD_EXIT: i64 = 97;
/// ssh(1) exits 255 when the connection itself failed.
const SSH_TRANSPORT_EXIT: i64 = 255;
const NODE_INSTALL_KEY_REDACTION: &str = "[REDACTED]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshNodeEnrollment {
    pub account_id: String,
    pub machine_record_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshNodeService {
    pub supervised: bool,
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Summary install.sh prints as its last JSON line with `--json`. Unknown fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshNodeInstallReport {
    pub cli_path: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrolled: Option<SshNodeEnrollment>,
    pub service: SshNodeService,
}

fn node_install_error(stage: &str, exit_code: Option<i64>, stderr_tail: String) -> IpcError {
    IpcError::new(
        IpcErrorCode::Custom(SSH_NODE_INSTALL_FAILED.to_string()),
        format!("Installing Ferryx over SSH failed during {stage}"),
    )
    .with_details(serde_json::json!({
        "stage": stage,
        "exitCode": exit_code,
        "stderrTail": stderr_tail,
    }))
}

/// Accepts an https origin, or http only for a loopback host. Returns it without a trailing slash.
pub(crate) fn validate_install_origin(origin: &str) -> Result<String, IpcError> {
    let invalid = |reason: &str| {
        IpcError::new(
            IpcErrorCode::InvalidArgument,
            "Account origin must be an https URL (http is allowed only for loopback)",
        )
        .with_details(serde_json::json!({"stage": "origin", "reason": reason}))
    };
    let parsed = url::Url::parse(origin.trim()).map_err(|_| invalid("unparseable"))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid("unexpectedComponent"));
    }
    let loopback = match parsed.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => return Err(invalid("missingHost")),
    };
    match parsed.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => return Err(invalid("scheme")),
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

/// The ssh stdin payload: the install key followed by one newline. The key never enters argv.
pub(crate) fn node_install_stdin(install_key: &str) -> Result<Vec<u8>, IpcError> {
    if install_key.is_empty() || install_key.chars().any(char::is_control) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "Install key must be a single non-empty line",
        )
        .with_details(serde_json::json!({"stage": "key"})));
    }
    let mut bytes = Vec::with_capacity(install_key.len() + 1);
    bytes.extend_from_slice(install_key.as_bytes());
    bytes.push(b'\n');
    Ok(bytes)
}

/// Remote POSIX command for a validated origin. install.sh is downloaded to a temp file (curl,
/// else wget) rather than piped into `sh -s`: a piped script would occupy sh's stdin, and
/// `--key-stdin` must read the key line from the ssh stdin instead.
pub(crate) fn node_install_command(origin: &str) -> String {
    let script_url = crate::ssh::direct::quote_posix(&format!("{origin}/install.sh"));
    let origin = crate::ssh::direct::quote_posix(origin);
    let download = NODE_INSTALL_DOWNLOAD_EXIT;
    format!(
        "tmp=$(mktemp 2>/dev/null || mktemp -t ferryx-install) || exit {download}; \
         trap 'rm -f \"$tmp\"' EXIT; \
         if command -v curl >/dev/null 2>&1; then curl -fsSL {script_url} -o \"$tmp\" || exit {download}; \
         elif command -v wget >/dev/null 2>&1; then wget -qO \"$tmp\" {script_url} || exit {download}; \
         else echo 'ferryx: curl or wget is required' >&2; exit {download}; fi; \
         FERRYX_ORIGIN={origin} sh \"$tmp\" --key-stdin --json"
    )
}

/// Redacts the key, then keeps the last [`NODE_INSTALL_STDERR_TAIL_BYTES`] bytes on a char boundary.
pub(crate) fn node_install_stderr_tail(stderr: &str, install_key: &str) -> String {
    let redacted = if install_key.is_empty() {
        stderr.to_string()
    } else {
        stderr.replace(install_key, NODE_INSTALL_KEY_REDACTION)
    };
    let mut start = redacted.len().saturating_sub(NODE_INSTALL_STDERR_TAIL_BYTES);
    while !redacted.is_char_boundary(start) {
        start += 1;
    }
    redacted[start..].to_string()
}

/// Maps a structured `bounded_output_with_stdin` error to `SSH_NODE_INSTALL_FAILED`.
pub(crate) fn node_install_failure(error: IpcError, install_key: &str) -> IpcError {
    let details = error.details.as_ref();
    let exit_code = details
        .and_then(|details| details.get("exitCode"))
        .and_then(serde_json::Value::as_i64);
    let source_stage = details
        .and_then(|details| details.get("stage"))
        .and_then(serde_json::Value::as_str);
    let stderr = details
        .and_then(|details| details.get("stderr"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let stage = match (source_stage, exit_code) {
        (Some("execution"), Some(SSH_TRANSPORT_EXIT)) => "transport",
        (Some("execution"), Some(NODE_INSTALL_DOWNLOAD_EXIT)) => "download",
        (Some("execution"), _) => "install",
        _ => "transport",
    };
    node_install_error(stage, exit_code, node_install_stderr_tail(stderr, install_key))
}

/// Parses the last stdout line that is a JSON object into the report.
pub(crate) fn parse_node_install_report(stdout: &[u8]) -> Result<SshNodeInstallReport, IpcError> {
    let text = std::str::from_utf8(stdout)
        .map_err(|_| node_install_error("parse", Some(0), String::new()))?;
    let last = text
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find_map(|line| match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value @ serde_json::Value::Object(_)) => Some(value),
            _ => None,
        })
        .ok_or_else(|| node_install_error("parse", Some(0), String::new()))?;
    serde_json::from_value(last).map_err(|_| node_install_error("parse", Some(0), String::new()))
}

#[cfg(feature = "desktop")]
/// Installs the Ferryx CLI on a stored SSH host and enrolls it with a one-time install key.
#[tauri::command]
pub async fn cmd_ssh_install_ferryx_node<R: Runtime>(
    app: AppHandle<R>,
    host_id: String,
    account_origin: String,
    install_key: String,
) -> Result<SshNodeInstallReport, IpcError> {
    let origin = validate_install_origin(&account_origin)?;
    let input = node_install_stdin(&install_key)?;
    let path = get_ssh_store_path(&app)?;
    let host =
        run_blocking(move || crate::ssh::projects::enabled_host(&path, &host_id)).await?;
    let plan = crate::ssh::direct::ssh_plan(&host, node_install_command(&origin), false)?;
    let output = crate::ssh::direct::bounded_output_with_stdin(&plan, NODE_INSTALL_TIMEOUT, input)
        .await
        .map_err(|error| node_install_failure(error, &install_key))?;
    parse_node_install_report(&output)
}

pub fn resolve_bundled_helper_binary(target_triple: &str) -> Result<PathBuf, IpcError> {
    let parsed_target = match target_triple {
        "x86_64-unknown-linux-gnu" => crate::ssh::helper_assets::HelperTarget::LinuxX86_64,
        "aarch64-unknown-linux-gnu" => crate::ssh::helper_assets::HelperTarget::LinuxAarch64,
        "x86_64-apple-darwin" => crate::ssh::helper_assets::HelperTarget::DarwinX86_64,
        "aarch64-apple-darwin" => crate::ssh::helper_assets::HelperTarget::DarwinAarch64,
        "x86_64-pc-windows-msvc" => crate::ssh::helper_assets::HelperTarget::WindowsX64,
        _ => {
            return Err(IpcError::new(
                IpcErrorCode::Unsupported,
                format!("Unsupported target triple for helper asset: '{target_triple}'"),
            ));
        }
    };

    let mut helper_dir_candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        // A staged daemon generation carries its own copy (`runtime_stage::HELPER_ASSETS_DIR`).
        if let Some(staged) = crate::daemon::runtime_stage::helper_assets_dir_near(&exe) {
            helper_dir_candidates.push(staged);
        }
        if let Some(exe_dir) = exe.parent() {
            helper_dir_candidates.push(exe_dir.join("helpers"));
            helper_dir_candidates.push(exe_dir.join("../Resources/helpers"));
            helper_dir_candidates.push(exe_dir.join("resources/helpers"));
        }
    }

    helper_dir_candidates.push(PathBuf::from("src-tauri/resources/helpers"));
    helper_dir_candidates.push(PathBuf::from("resources/helpers"));
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        helper_dir_candidates.push(PathBuf::from(manifest_dir).join("resources/helpers"));
    }

    for dir in &helper_dir_candidates {
        if dir.join("manifest.json").is_file() {
            let resolved = crate::ssh::helper_assets::resolve_current_helper_asset(dir, parsed_target)?;
            return Ok(resolved.binary_path);
        }
    }

    Err(IpcError::new(
        IpcErrorCode::CliExecutableNotFound,
        format!("Bundled helper binary for target '{target_triple}' not found"),
    ))
}

#[cfg(feature = "desktop")]
pub fn bundled_helper_path<R: Runtime>(
    app: &AppHandle<R>,
    target_triple: &str,
) -> Result<PathBuf, IpcError> {
    let parsed_target = match target_triple {
        "x86_64-unknown-linux-gnu" => crate::ssh::helper_assets::HelperTarget::LinuxX86_64,
        "aarch64-unknown-linux-gnu" => crate::ssh::helper_assets::HelperTarget::LinuxAarch64,
        "x86_64-apple-darwin" => crate::ssh::helper_assets::HelperTarget::DarwinX86_64,
        "aarch64-apple-darwin" => crate::ssh::helper_assets::HelperTarget::DarwinAarch64,
        "x86_64-pc-windows-msvc" => crate::ssh::helper_assets::HelperTarget::WindowsX64,
        _ => {
            return Err(IpcError::new(
                IpcErrorCode::Unsupported,
                format!("Unsupported target triple for helper asset: '{target_triple}'"),
            ));
        }
    };

    let mut helper_dir_candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        helper_dir_candidates.push(resource_dir.join("helpers"));
        helper_dir_candidates.push(resource_dir.join("resources").join("helpers"));
    }

    for dir in &helper_dir_candidates {
        if dir.join("manifest.json").is_file() {
            let resolved = crate::ssh::helper_assets::resolve_current_helper_asset(dir, parsed_target)?;
            return Ok(resolved.binary_path);
        }
    }

    resolve_bundled_helper_binary(target_triple)
}

#[cfg(feature = "desktop")]
pub fn bundled_helper_version<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("helpers").join("manifest.json"));
        candidates.push(
            resource_dir
                .join("resources")
                .join("helpers")
                .join("manifest.json"),
        );
    }
    candidates.push(PathBuf::from("src-tauri/resources/helpers/manifest.json"));
    candidates.push(PathBuf::from("resources/helpers/manifest.json"));
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        candidates.push(
            PathBuf::from(manifest_dir)
                .join("resources/helpers")
                .join("manifest.json"),
        );
    }

    for candidate in candidates {
        if candidate.is_file() {
            if let Ok(bytes) = std::fs::read(&candidate) {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(v) = val.get("helperVersion").and_then(|v| v.as_str()) {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

pub async fn resolve_remote_target_triple(
    host: &SshHost,
    env: &crate::ssh::runtime::RemoteEnvironment,
) -> Result<String, IpcError> {
    match env.platform {
        crate::ssh::runtime::RemotePlatform::Windows => Ok("x86_64-pc-windows-msvc".to_string()),
        crate::ssh::runtime::RemotePlatform::Posix => {
            let script = "uname -s; uname -m";
            let plan = crate::ssh::direct::ssh_plan(host, env.executor.command(script), false)?;
            let output =
                crate::ssh::direct::bounded_output(&plan, std::time::Duration::from_secs(3))
                    .await
                    .map_err(|e| {
                        IpcError::new(
                            IpcErrorCode::Unsupported,
                            format!("Failed to determine remote architecture: {e}"),
                        )
                    })?;
            let text = std::str::from_utf8(&output).map_err(|_| {
                IpcError::new(
                    IpcErrorCode::Unsupported,
                    "Remote architecture probe returned non-UTF8 output",
                )
            })?;
            let lines: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let (os, arch) = match lines.as_slice() {
                [os, arch, ..] => (*os, *arch),
                [os] => (*os, "x86_64"),
                _ => {
                    return Err(IpcError::new(
                        IpcErrorCode::Unsupported,
                        "Could not detect remote operating system or architecture for provisioning",
                    ));
                }
            };

            let target = crate::ssh::helper_assets::resolve_target_from_probe(os, arch)?;
            Ok(target.triple().to_string())
        }
    }
}

pub async fn ensure_qualified_ssh_helper(
    host: &crate::ssh::SshHost,
    environment: &crate::ssh::runtime::RemoteEnvironment,
) -> Result<crate::ssh::helper_setup::HelperLocation, IpcError> {
    let location = crate::ssh::helper_setup::qualified_location(
        host,
        environment,
        crate::ssh::helper_runtime::process::HELPER_VERSION,
    )?;

    if crate::ssh::helper_setup::probe_ready_at(host, environment, &location).await
        == crate::ssh::helper_setup::HelperProbeState::Installed
    {
        return Ok(location);
    }

    let target_triple = resolve_remote_target_triple(host, environment).await?;
    let binary_path = crate::ipc::run_blocking(move || {
        resolve_bundled_helper_binary(&target_triple)
    })
    .await?;

    crate::ssh::helper_setup::install(host, environment, &location, &binary_path).await?;
    crate::ssh::helper_setup::ensure_started(host, environment, &location).await?;
    Ok(location)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_sync_tailscale<R: Runtime>(app: AppHandle<R>) -> Result<SshTailscaleSyncResult, IpcError> {
    let _sync = SSH_TAILSCALE_SYNC.lock().await;
    let path = get_ssh_store_path(&app)?;
    match crate::remote::discovery::tailscale::TailscaleDiscoveryProvider::new().discover_ssh_status_async().await {
        Err(crate::remote::discovery::tailscale::TailscaleDiscoveryError::Timeout) => {
            let hosts = run_blocking(move || Ok(load_store(&path)?.hosts)).await?;
            Ok(SshTailscaleSyncResult { hosts, targets: Vec::new(), discovery_error: Some(IpcError::new(IpcErrorCode::Timeout, "Tailscale status discovery timed out").with_details(serde_json::json!({"stage":"discovery"}))) })
        }
        Err(error) => {
            let hosts = run_blocking(move || Ok(load_store(&path)?.hosts)).await?;
            let (code, stage) = match &error { crate::remote::discovery::tailscale::TailscaleDiscoveryError::Command(_) => (IpcErrorCode::CliExecutableNotFound, "command"), crate::remote::discovery::tailscale::TailscaleDiscoveryError::Parse(_) => (IpcErrorCode::InvalidRequest, "parse"), crate::remote::discovery::tailscale::TailscaleDiscoveryError::Timeout => unreachable!() };
            Ok(SshTailscaleSyncResult { hosts, targets: Vec::new(), discovery_error: Some(IpcError::new(code, error.to_string()).with_details(serde_json::json!({"stage":stage}))) })
        }
        Ok(status) => sync_tailscale_peers(path, status).await,
    }
}

async fn sync_tailscale_peers(path: PathBuf, status: crate::remote::discovery::tailscale::TailscaleSshStatus) -> Result<SshTailscaleSyncResult, IpcError> {
    let config_hosts = run_blocking(|| Ok(system_ssh_config_for_sync())).await?;
    sync_tailscale_peers_with(path, status, config_hosts, sync_tailscale_target).await
}

async fn sync_tailscale_peers_with<F, Fut>(path: PathBuf, status: crate::remote::discovery::tailscale::TailscaleSshStatus, config_snapshot: Option<Vec<SshHost>>, provision: F) -> Result<SshTailscaleSyncResult, IpcError>
where
    F: Fn(SshHost, Option<String>) -> Fut + Clone,
    Fut: std::future::Future<Output = Result<SshTargetSummary, IpcError>> + Send + 'static,
{
    let self_ip = status.self_ipv4.clone();
    let peers = status.peers;
    let config_hosts = config_snapshot.clone().unwrap_or_default();
    let provisioning_peers: Vec<_> = peers.iter().filter(|peer| peer.online && !matches!(peer.os.as_deref().map(str::to_ascii_lowercase).as_deref(), Some("android" | "ios" | "ipados"))).cloned().collect();
    let merge_path = path.clone();
    let merge_peers = peers.clone();
    let merge_config_known = config_snapshot;
    let merge_self_ip = self_ip;
    let hosts = run_blocking(move || {
        let _guard = lock_ssh_store();
        let mut store = load_store(&merge_path)?;
        merge_tailscale_peers(&mut store, &merge_peers, &config_hosts);
        retain_vanished_hosts(&mut store, &merge_peers, merge_self_ip.as_deref(), merge_config_known.as_deref());
        dedupe_tailscale_rows(&mut store);
        save_store(&merge_path, &store)?;
        Ok(store.hosts)
    }).await?;
    use tokio::task::JoinSet;
    let mut targets = peers.iter().filter(|peer| !peer.online || matches!(peer.os.as_deref().map(str::to_ascii_lowercase).as_deref(), Some("android" | "ios" | "ipados"))).filter_map(|peer| {
        let host = hosts.iter().find(|host| host.id == format!("tailscale:{}", peer.node_id) || tailnet_host_matches(host, peer))?.clone();
        let diagnostic = if !peer.online { IpcError::new(IpcErrorCode::HostUnavailable, "Tailscale peer is offline").with_details(serde_json::json!({"stage":"discovery","backendState":"offline"})) }
            else { IpcError::new(IpcErrorCode::Unsupported, "Unsupported Tailscale peer operating system").with_details(serde_json::json!({"stage":"platform","os":peer.os})) };
        let summary = SshTargetSummary { host, reachable: false, last_error: Some(diagnostic.message.clone()), environment: None, helper: Some(crate::ssh::helper_setup::HelperProbeState::Unknown), diagnostic: Some(diagnostic), checked_at: now_millis() };
        (summary.host.disabled != Some(true)).then_some(summary)
    }).collect::<Vec<_>>();
    let mut set = JoinSet::new();
    let mut pending = provisioning_peers.into_iter().filter_map(|peer| {
        let host = hosts.iter().find(|host| host.id == format!("tailscale:{}", peer.node_id) || tailnet_host_matches(host, &peer))?;
        (host.disabled != Some(true)).then_some((host.clone(), peer.os))
    });
    for _ in 0..4 {
        if let Some((host, os)) = pending.next() {
            set.spawn(provision(host, os));
        }
    }
    while let Some(result) = set.join_next().await {
        match result { Ok(Ok(target)) => targets.push(target), Ok(Err(error)) => return Err(error), Err(error) => return Err(IpcError::new(IpcErrorCode::InternalError, format!("Tailscale target task failed: {error}"))) }
        if let Some((host, os)) = pending.next() {
            set.spawn(provision(host, os));
        }
    }
    let (hosts, targets) = run_blocking(move || {
        let _guard = lock_ssh_store();
        let current = load_store(&path)?;
        let valid: Vec<_> = targets.into_iter().filter(|target| current.hosts.iter().any(|host| host.id == target.host.id && host == &target.host)).collect();
        Ok((current.hosts, valid))
    }).await?;
    Ok(SshTailscaleSyncResult { hosts, targets, discovery_error: None })
}

// The aliases currently declared in the user's `~/.ssh/config`.
//
// `None` means the file could not be read at all, which is not the same as "the user has no
// aliases": pruning config rows against an unreadable file would erase every imported alias.
fn system_ssh_config_for_sync() -> Option<Vec<SshHost>> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let text = std::fs::read_to_string(PathBuf::from(home).join(".ssh/config")).ok()?;
    Some(crate::ssh::config::import_aliases(&parse_ssh_config(&text), &[]))
}

async fn sync_tailscale_target(host: SshHost, os: Option<String>) -> Result<SshTargetSummary, IpcError> {
    let platform = match os.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("windows") => Some(crate::ssh::runtime::RemotePlatform::Windows),
        Some("linux" | "darwin" | "macos" | "freebsd" | "openbsd") => Some(crate::ssh::runtime::RemotePlatform::Posix),
        Some(_) => return Ok(SshTargetSummary { host, reachable: false, last_error: Some("Unsupported Tailscale peer operating system".into()), environment: None, helper: Some(crate::ssh::helper_setup::HelperProbeState::Unknown), diagnostic: Some(IpcError::new(IpcErrorCode::Unsupported, "Unsupported Tailscale peer operating system").with_details(serde_json::json!({"stage":"platform"}))), checked_at: now_millis() }),
        None => None,
    };
    let probe_host = host.clone();
    let provision_host = host.clone();
    sync_tailscale_target_with(host,
        move || async move { crate::ssh::runtime::detect_for(&probe_host, platform).await },
        move |environment| async move {
            let location = ensure_qualified_ssh_helper(&provision_host, &environment).await?;
            crate::ssh::helper_setup::ensure_started(&provision_host, &environment, &location).await
        }).await
}

async fn sync_tailscale_target_with<D, DFut, P, PFut>(
    host: SshHost,
    detect: D,
    provision: P,
) -> Result<SshTargetSummary, IpcError>
where
    D: FnOnce() -> DFut,
    DFut: std::future::Future<Output = Result<crate::ssh::runtime::RemoteEnvironment, IpcError>>,
    P: FnOnce(crate::ssh::runtime::RemoteEnvironment) -> PFut,
    PFut: std::future::Future<Output = Result<(), IpcError>>,
{
    let environment = match detect().await {
        Ok(environment) => environment,
        Err(error) => return Ok(SshTargetSummary { host, reachable: false, last_error: Some(error.message.clone()), environment: None, helper: None, diagnostic: Some(error), checked_at: now_millis() }),
    };
    let helper = match provision(environment.clone()).await {
        Ok(()) => Some(crate::ssh::helper_setup::HelperProbeState::Installed),
        Err(error) => {
            return Ok(SshTargetSummary { host, reachable: true, last_error: Some(error.message.clone()), environment: Some(environment), helper: Some(crate::ssh::helper_setup::HelperProbeState::Unknown), diagnostic: Some(error), checked_at: now_millis() });
        }
    };
    Ok(SshTargetSummary { host, reachable: true, last_error: None, environment: Some(environment), helper, diagnostic: None, checked_at: now_millis() })
}

fn merge_tailscale_peers(store: &mut SshHostStore, peers: &[crate::remote::discovery::tailscale::TailscaleSshPeer], config_hosts: &[SshHost]) {
    for peer in peers {
        let id = format!("tailscale:{}", peer.node_id);
        if store.tombstones.contains(&id) { continue; }
        if [Some(peer.ipv4.as_str()), Some(peer.hostname.as_str()), peer.dns_name.as_deref()]
            .into_iter().flatten().any(|name| store.tombstones.contains(&format!("tailscale-host:{}", name.to_ascii_lowercase()))) { continue; }
        let saved = store.hosts.iter().find(|host| host.id == id)
            .or_else(|| store.hosts.iter().find(|host| tailnet_host_matches(host, peer)));
        let matched = saved.or_else(|| config_hosts.iter().find(|host| tailnet_host_matches(host, peer)));
        let host = if let Some(existing) = matched {
            let mut host = existing.clone();
            if store.tombstones.contains(&host.key()) || store.tombstones.contains(&id) { continue; }
            if saved.is_none() {
                host.id = id.clone();
                host.source = crate::ssh::SshHostSource::Tailscale;
            }
            if host.source == crate::ssh::SshHostSource::Tailscale {
                host.hostname = peer.ipv4.clone();
            }
            host
        } else {
            SshHost { id: id.clone(), label: peer.dns_name.clone().unwrap_or_else(|| peer.hostname.clone()), hostname: peer.ipv4.clone(), username: None, port: None, identity_file: None, jump_host: None, source: crate::ssh::SshHostSource::Tailscale, auth_method: crate::ssh::SshAuthMethod::Agent, disabled: None, shell: None }
        };
        if let Some(index) = store.hosts.iter().position(|current| current.id == host.id) {
            store.hosts[index] = host;
        } else if !store.hosts.iter().any(|current| current.id == id) {
            store.hosts.push(host);
        }
    }
}

// Drops rows whose upstream no longer reports the machine.
//
// Merging alone only ever adds, so every host that once existed kept sitting in the picker
// forever: a laptop that left the tailnet, an offline peer, an alias deleted from
// ~/.ssh/config. Nothing on the row itself distinguishes "configured but asleep" from "gone",
// so the upstream truth decides:
//
// - Manual rows are the user's own entries and are never pruned.
// - Config rows survive while their alias is still declared, while a live peer claims them, or
//   when the config could not be read at all (None) - an unreadable file is not an empty one.
// - Tailscale rows survive only while an online peer still answers to them; a disabled row is
//   kept because disabling is an explicit "keep this, do not use it" decision. This machine's own
//   tailnet address also survives: a row naming our own IP describes us, not a vanished peer. So
//   does a row whose address or label the user still wrote in ~/.ssh/config, and every row when
//   the config could not be read: discovery keeps the peer id when it adopts a user alias, so
//   "omarchy2" is stored as a Tailscale row even though the user owns the entry.
fn retain_vanished_hosts(
    store: &mut SshHostStore,
    peers: &[crate::remote::discovery::tailscale::TailscaleSshPeer],
    self_ipv4: Option<&str>,
    config_hosts: Option<&[SshHost]>,
) {
    let config_ids: std::collections::HashSet<&str> = config_hosts
        .iter()
        .flat_map(|hosts| hosts.iter().map(|host| host.id.as_str()))
        .collect();
    let live_peer = |host: &SshHost| {
        peers.iter().any(|peer| {
            peer.online
                && (host.id == format!("tailscale:{}", peer.node_id) || tailnet_host_matches(host, peer))
        })
    };
    store.hosts.retain(|host| match host.source {
        crate::ssh::SshHostSource::Manual => true,
        crate::ssh::SshHostSource::Config => {
            config_hosts.is_none() || config_ids.contains(host.id.as_str()) || live_peer(host)
        }
        crate::ssh::SshHostSource::Tailscale => {
            // An unreadable config must not be treated as an empty one: without it we cannot tell
            // a user-written alias from stale discovery junk, and dropping the row can delete a
            // host the user still expects to reach.
            config_hosts.is_none()
                || host.disabled == Some(true)
                || live_peer(host)
                || self_ipv4.is_some_and(|ip| host.hostname.eq_ignore_ascii_case(ip))
                || config_hosts.is_some_and(|entries| config_owns(host, entries))
        }
    });
}

// True when the user's ~/.ssh/config still names this machine, by address or by alias.
//
// A discovered row can also stand for a config host: merge_tailscale_peers keeps the peer's id
// when it adopts the alias and login the user wrote, so "omarchy2" or "dots" is stored with a
// tailscale id even though the user owns the entry. Pruning it because the tailnet does not
// report the box right now would delete a host the user still reaches by name, so config
// ownership outranks liveness.
fn config_owns(host: &SshHost, config_hosts: &[SshHost]) -> bool {
    config_hosts.iter().any(|entry| {
        entry.hostname.eq_ignore_ascii_case(&host.hostname)
            || entry.label.eq_ignore_ascii_case(&host.label)
    })
}

// Collapses several rows that describe the same machine into one.
//
// A single box is regularly present three times - once hand-added, once imported from
// ~/.ssh/config, once discovered over Tailscale - because each source stores its own row and
// merge updates rows in place instead of removing the losers. The surviving row is the richest
// identity (Manual, then Config, then Tailscale), because a hand-written alias carries the label
// and credentials a bare peer address does not.
//
// Two rows imported from ~/.ssh/config are NEVER folded into each other, even when they share an
// address and a login: those are aliases the user wrote on purpose. A real config carries
// "maho-win", "maho-scalper" and "maho-steam" on one machine, and collapsing them would delete two
// of the user's own entries the first time Tailscale discovery ran. Only a discovered row is ever
// absorbed, and only into a stronger row.
//
// Rows with two different explicit usernames are kept apart: "box" and "box-root" are different
// logins to the same address, not duplicates.
fn dedupe_tailscale_rows(store: &mut SshHostStore) {
    // Only a discovered row is ever folded away. A hand-added row and a ~/.ssh/config alias are
    // the user's own entries: several aliases may name one machine on purpose, so collapsing them
    // would delete rows the user wrote.
    //
    // A discovered row is folded when *any* row the user owns names the same machine. Testing only
    // the rows kept so far made the outcome depend on insertion order, so a refresh that stored
    // discovery output first kept a duplicate that an earlier pass had removed.
    let hosts = std::mem::take(&mut store.hosts);
    let discovered = |host: &SshHost| matches!(host.source, crate::ssh::SshHostSource::Tailscale);
    let mut kept: Vec<SshHost> = Vec::with_capacity(hosts.len());
    for index in 0..hosts.len() {
        let host = &hosts[index];
        if !discovered(host) {
            kept.push(host.clone());
            continue;
        }
        let shadowed_by_user = hosts.iter().enumerate().any(|(other, candidate)| {
            other != index && !discovered(candidate) && same_machine(candidate, host)
        });
        let shadowed_by_kept = kept.iter().any(|existing| same_machine(existing, host));
        if !shadowed_by_user && !shadowed_by_kept {
            kept.push(host.clone());
        }
    }
    store.hosts = kept;
}

fn same_machine(left: &SshHost, right: &SshHost) -> bool {
    left.hostname.eq_ignore_ascii_case(&right.hostname)
        && match (left.username.as_deref(), right.username.as_deref()) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
}

fn tailnet_host_matches(host: &SshHost, peer: &crate::remote::discovery::tailscale::TailscaleSshPeer) -> bool {
    let direct = [Some(peer.ipv4.as_str()), Some(peer.hostname.as_str()), peer.dns_name.as_deref()];
    if direct.iter().flatten().any(|name| host.hostname.eq_ignore_ascii_case(name)) { return true; }
    let Some(dns) = peer.dns_name.as_deref() else { return false; };
    let short_dns = dns.split('.').next().unwrap_or(dns);
    host.hostname.parse::<std::net::Ipv4Addr>().is_err()
        && !host.hostname.contains('.')
        && host.hostname.eq_ignore_ascii_case(short_dns)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperUpdateState {
    pub installed: bool,
    pub remote_version: Option<String>,
    pub bundled_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled_path: Option<PathBuf>,
    /// Backend-computed upgrade decision so the UI never re-implements version
    /// comparison. `None` when no probe ran (runtime detection failed) or no
    /// bundled version is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<crate::ssh::helper_setup::HelperUpgradeDecision>,
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_provision_helper(
    host: SshHost,
    local_binary: PathBuf,
) -> Result<crate::ssh::helper_setup::HelperLocation, IpcError> {
    let environment = crate::ssh::runtime::detect(&host).await?;
    crate::ssh::helper_setup::provision_qualified(
        &host,
        &environment,
        &local_binary,
        crate::ssh::helper_runtime::process::HELPER_VERSION,
    )
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_helper_update_state<R: Runtime>(
    app: AppHandle<R>,
    host: SshHost,
) -> Result<HelperUpdateState, IpcError> {
    let bundled_ver = bundled_helper_version(&app);

    let env = match crate::ssh::runtime::detect(&host).await {
        Ok(e) => e,
        Err(_) => {
            return Ok(HelperUpdateState {
                installed: false,
                remote_version: None,
                bundled_version: bundled_ver,
                bundled_path: None,
                // No probe ran: claim nothing actionable, matching the honest tri-state.
                decision: None,
            });
        }
    };

    let probe = crate::ssh::helper_setup::probe_ready(&host, &env).await;
    let installed = probe == crate::ssh::helper_setup::HelperProbeState::Installed;

    let remote_version = if installed {
        crate::ssh::helper_setup::installed_version(&host, &env).await
    } else {
        None
    };

    let bundled_path = match resolve_remote_target_triple(&host, &env).await {
        Ok(triple) => bundled_helper_path(&app, &triple).ok(),
        Err(_) => None,
    };

    let decision = bundled_ver.as_deref().map(|bundled| {
        crate::ssh::helper_setup::decide_helper_upgrade(
            installed,
            remote_version.as_deref(),
            bundled,
        )
    });

    Ok(HelperUpdateState {
        installed,
        remote_version,
        bundled_version: bundled_ver,
        decision,
        bundled_path,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshClipboardImagePaste {
    pub remote_path: String,
    pub byte_length: usize,
}

#[cfg(feature = "desktop")]
/// Copies the clipboard image to the SSH host that owns `workspace_id` so the remote agent can
/// open it, and answers with the path to paste. `Ok(None)` means the clipboard held nothing the
/// remote could use, which leaves the caller on its local paste chord.
#[tauri::command]
pub async fn cmd_ssh_paste_clipboard_image<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
) -> Result<Option<SshClipboardImagePaste>, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let (_, host) =
        run_blocking(move || crate::ssh::projects::resolve(&store, &workspace_id)).await?;
    let Some(image) = crate::clipboard_image::read_clipboard_image_for_app(&app).await? else {
        return Ok(None);
    };
    let byte_length = image.bytes.len();
    if byte_length > crate::clipboard_image::MAX_CLIPBOARD_IMAGE_BYTES {
        return Err(IpcError::new(
            IpcErrorCode::Unsupported,
            format!(
                "Clipboard image is {} MiB, above the {} MiB a remote paste may transfer",
                byte_length / (1024 * 1024),
                crate::clipboard_image::MAX_CLIPBOARD_IMAGE_BYTES / (1024 * 1024)
            ),
        ));
    }
    let file_name = format!("{}.{}", uuid::Uuid::new_v4(), image.extension);
    let remote_path = crate::ssh::direct::upload_temp_file(&host, &file_name, image.bytes).await?;
    Ok(Some(SshClipboardImagePaste {
        remote_path,
        byte_length,
    }))
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_list_remote_worktrees<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
) -> Result<Vec<crate::ssh::worktree::RemoteWorktree>, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let id = workspace_id.clone();
    let (project, host) = run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
    let environment = crate::ssh::runtime::detect_for(&host, project.platform).await?;
    crate::ssh::worktree::list_remote(&host, &environment, &project.repo_root).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_create_remote_worktree<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    slug: String,
    base_ref: Option<String>,
) -> Result<crate::ssh::worktree::RemoteWorktree, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let id = workspace_id.clone();
    let (project, host) = run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
    let environment = crate::ssh::runtime::detect_for(&host, project.platform).await?;
    let ws_segment = crate::ssh::worktree::derive_ws_segment(&workspace_id)?;
    let wt_path = crate::ssh::worktree::remote_worktree_path(
        environment.platform,
        &project.repo_root,
        &ws_segment,
        &slug,
    );
    crate::ssh::worktree::create_remote(
        &host,
        &environment,
        &project.repo_root,
        &ws_segment,
        &slug,
        base_ref.as_deref(),
        &wt_path,
    )
    .await?;
    let worktrees =
        crate::ssh::worktree::list_remote(&host, &environment, &project.repo_root).await?;
    let norm_wt_path = wt_path.replace('\\', "/").trim_end_matches('/').to_string();
    let created = worktrees
        .into_iter()
        .find(|wt| {
            wt.path == wt_path || wt.path.replace('\\', "/").trim_end_matches('/') == norm_wt_path
        })
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Created worktree not found at {wt_path} after creation"),
            )
        })?;

    let identity = crate::worktree::WorktreeIdentity {
        ws_id: workspace_id.clone(),
        slug,
    };
    crate::ipc::worktree::emit_worktree_changed(
        &app,
        workspace_id,
        identity,
        crate::ipc::worktree::WorktreeChangeKind::Created,
    )?;
    Ok(created)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_preview_remote_worktree_delete<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    path: String,
) -> Result<crate::worktree::BranchDeletionPreview, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let (project, host) = crate::ipc::run_blocking(move || {
        crate::ssh::projects::resolve(&store, &workspace_id)
    }).await?;
    let environment = crate::ssh::runtime::detect_for(&host, project.platform).await?;
    crate::ssh::worktree::validate_path_inside_root(environment.platform, &project.repo_root, &path)?;
    crate::ssh::worktree::preview_remote_delete(&host, &environment, &project.repo_root, &path).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_ssh_delete_remote_worktree<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    path: String,
    force: Option<bool>,
) -> Result<(), IpcError> {
    let store = get_ssh_store_path(&app)?;
    let id = workspace_id.clone();
    let (project, host) = run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
    let environment = crate::ssh::runtime::detect_for(&host, project.platform).await?;
    crate::ssh::worktree::validate_path_inside_root(
        environment.platform,
        &project.repo_root,
        &path,
    )?;
    crate::ssh::worktree::remove_remote(
        &host,
        &environment,
        &project.repo_root,
        &path,
        force.unwrap_or(false),
    )
    .await?;
    // Both `<dir>/<ws>/<slug>` and the legacy `<dir>/wt-<slug>` layout name their slug.
    let slug = crate::worktree::naming::slug_from_worktree_path(&path).unwrap_or_default();
    let identity = crate::worktree::WorktreeIdentity {
        ws_id: workspace_id.clone(),
        slug,
    };
    crate::ipc::worktree::emit_worktree_changed(
        &app,
        workspace_id,
        identity,
        crate::ipc::worktree::WorktreeChangeKind::Deleted,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::{SshAuthMethod, SshHostSource};

    #[test]
    fn store_round_trips_through_json() {
        let store = SshHostStore {
            hosts: vec![SshHost {
                id: "h1".into(),
                label: "win".into(),
                hostname: "maho-win".into(),
                username: Some("sook".into()),
                port: Some(2200),
                identity_file: None,
                jump_host: None,
                source: SshHostSource::Config,
                auth_method: SshAuthMethod::Agent,
                disabled: None,
                shell: None,
            }],
            tombstones: vec!["maho-win:22".into()],
        };
        let text = serde_json::to_string(&store).expect("serialize");
        let back: SshHostStore = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.hosts.len(), 1);
        assert_eq!(back.tombstones, vec!["maho-win:22".to_string()]);
    }

    #[test]
    fn default_store_is_empty() {
        let store: SshHostStore = serde_json::from_str("{}").expect("deserialize");
        assert!(store.hosts.is_empty());
        assert!(store.tombstones.is_empty());
    }

    #[test]
    fn resilient_store_keeps_latest_duplicate_host_id() {
        let json = r#"{
            "hosts": [
                {
                    "id": "ssh-build",
                    "label": "Windows",
                    "hostname": "maho-win",
                    "source": "config",
                    "authMethod": "agent"
                },
                {
                    "id": "ssh-build",
                    "label": "Linux",
                    "hostname": "omarchy",
                    "source": "config",
                    "authMethod": "agent"
                }
            ]
        }"#;
        let store: SshHostStore = serde_json::from_str(json).expect("deserialize duplicate ids");
        assert_eq!(store.hosts.len(), 1);
        assert_eq!(store.hosts[0].label, "Linux");
        assert_eq!(store.hosts[0].hostname, "omarchy");
    }

    #[test]
    fn import_preserves_corrupt_store_bytes() {
        let dir = tempfile::tempdir().expect("temporary store");
        let path = dir.path().join("ssh_hosts.json");
        let original = b"{\"hosts\": [\n  {\"id\": \"saved-host\"},";
        std::fs::write(&path, original).expect("seed corrupt store");

        let result = import_config_into_store(&path, "Host new-box\n  HostName new.example\n");
        let saved = std::fs::read(&path).expect("read inventory after import");
        assert_eq!(
            (
                result.as_ref().err().map(|error| error.code.clone()),
                saved.as_slice()
            ),
            (Some(IpcErrorCode::ParseError), original.as_slice()),
        );
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn import_creates_missing_store() {
        let dir = tempfile::tempdir().expect("temporary store");
        let path = dir.path().join("new-data").join("ssh_hosts.json");
        assert!(!path.exists());

        let hosts = import_config_into_store(&path, "Host new-box\n  HostName new.example\n")
            .expect("import into missing store");
        let saved: SshHostStore =
            serde_json::from_slice(&std::fs::read(&path).expect("read new store"))
                .expect("parse new store");
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "new.example");
        assert_eq!(saved.hosts, hosts);
        assert!(saved.tombstones.is_empty());
    }

    #[test]
    fn explicit_import_restores_a_previously_deleted_config_host() {
        let dir = tempfile::tempdir().expect("temporary store");
        let path = dir.path().join("ssh_hosts.json");
        save_store(
            &path,
            &SshHostStore {
                hosts: Vec::new(),
                tombstones: vec!["dev@dev.example:22".into(), "other.example:22".into()],
            },
        )
        .expect("seed store");

        let hosts =
            import_config_into_store(&path, "Host dev-box\n  HostName dev.example\n  User dev\n")
                .expect("import config");

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].label, "dev-box");
        let saved = load_store(&path).expect("load saved store");
        assert_eq!(saved.hosts, hosts);
        assert_eq!(saved.tombstones, vec!["other.example:22"]);
    }

    #[test]
    fn repeated_import_preserves_existing_hosts_without_duplicates() {
        let dir = tempfile::tempdir().expect("temporary store");
        let path = dir.path().join("ssh_hosts.json");
        let config = "Host dev-box\n  HostName dev.example\n  User dev\n";
        let mut hosts = crate::ssh::config::import_aliases(&parse_ssh_config(config), &[]);
        hosts[0].label = "My development machine".into();
        save_store(
            &path,
            &SshHostStore {
                hosts: hosts.clone(),
                tombstones: Vec::new(),
            },
        )
        .expect("seed store");

        let imported = import_config_into_store(&path, config).expect("import config");

        assert_eq!(imported, hosts);
        assert_eq!(load_store(&path).expect("load saved store").hosts, hosts);
    }

    #[test]
    fn changed_config_alias_replaces_endpoint_without_duplicate_id() {
        let dir = tempfile::tempdir().expect("temporary store");
        let path = dir.path().join("ssh_hosts.json");
        let windows = "Host build\n  HostName maho-win\n";
        let linux = "Host build\n  HostName omarchy\n";

        let first = import_config_into_store(&path, windows).expect("import windows endpoint");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].hostname, "maho-win");

        let second = import_config_into_store(&path, linux).expect("replace with linux endpoint");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].id, first[0].id);
        assert_eq!(second[0].hostname, "omarchy");
        assert_eq!(load_store(&path).expect("load saved store").hosts, second);
    }

    #[test]
    fn explicit_config_file_is_read_and_parsed_from_its_own_path() {
        let dir = tempfile::tempdir().expect("temporary dir");
        let path = dir.path().join("work-ssh-config");
        std::fs::write(
            &path,
            "Host work-box\n  HostName work.example\n  User dev\n",
        )
        .expect("write config");

        let result = read_ssh_config_file(&path).expect("read config");

        assert!(result.exists);
        assert_eq!(result.path, path.to_string_lossy());
        assert_eq!(result.hosts.len(), 1);
        assert_eq!(result.hosts[0].label, "work-box");
        assert_eq!(result.hosts[0].hostname, "work.example");
        assert_eq!(result.hosts[0].username, Some("dev".into()));
    }

    #[test]
    fn missing_config_file_reports_absence_instead_of_failing() {
        let dir = tempfile::tempdir().expect("temporary dir");
        let path = dir.path().join("absent-config");

        let result = read_ssh_config_file(&path).expect("read config");

        assert!(!result.exists);
        assert!(result.hosts.is_empty());
        assert!(result.raw_text.is_empty());
    }

    #[test]
    fn expand_tilde_path_resolves_home_and_subpaths() {
        let home = Path::new("/Users/testuser");
        assert_eq!(
            expand_tilde_path("~", Some(home)),
            PathBuf::from("/Users/testuser")
        );
        assert_eq!(
            expand_tilde_path("~/work/ssh-config", Some(home)),
            PathBuf::from("/Users/testuser/work/ssh-config")
        );
        assert_eq!(
            expand_tilde_path("~\\work\\ssh-config", Some(home)),
            PathBuf::from("/Users/testuser/work/ssh-config")
        );
        assert_eq!(
            expand_tilde_path("/var/ssh/config", Some(home)),
            PathBuf::from("/var/ssh/config")
        );
        assert_eq!(
            expand_tilde_path("~/work/ssh-config", None),
            PathBuf::from("~/work/ssh-config")
        );
    }

    #[test]
    fn system_config_result_parses_sample_content() {
        let sample = "\
Host dev-box
    HostName 10.0.0.1
    User ubuntu
    Port 2222
";
        let parsed = parse_ssh_config(sample);
        let hosts = crate::ssh::config::import_aliases(&parsed, &[]);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].label, "dev-box");
        assert_eq!(hosts[0].hostname, "10.0.0.1");
        assert_eq!(hosts[0].username, Some("ubuntu".into()));
        assert_eq!(hosts[0].port, Some(2222));
    }

    #[test]
    fn resilient_store_skips_malformed_host_and_retains_valid() {
        let json = r#"{
            "hosts": [
                {
                    "invalidField": "completely broken entry",
                    "port": "not-a-number"
                },
                {
                    "id": "valid-1",
                    "name": "omaki",
                    "host": "100.91.254.71",
                    "user": "indo",
                    "port": 22
                }
            ],
            "tombstones": []
        }"#;
        let store: SshHostStore = serde_json::from_str(json).expect("deserialize resilient store");
        assert_eq!(store.hosts.len(), 1);
        assert_eq!(store.hosts[0].id, "valid-1");
        assert_eq!(store.hosts[0].label, "omaki");
        assert_eq!(store.hosts[0].hostname, "100.91.254.71");
        assert_eq!(store.hosts[0].username.as_deref(), Some("indo"));
    }

    #[test]
    fn save_store_invalidates_memoized_host_store() {
        let dir = tempfile::tempdir().expect("temporary store");
        let path = dir.path().join("ssh_hosts.json");
        let host = SshHost {
            id: "h1".into(),
            label: "Host 1".into(),
            hostname: "127.0.0.1".into(),
            username: None,
            port: None,
            identity_file: None,
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
            shell: None,
        };
        save_store(
            &path,
            &SshHostStore {
                hosts: vec![host.clone()],
                tombstones: vec![],
            },
        )
        .expect("initial save");

        let read1 = crate::ssh::projects::enabled_host(&path, "h1").expect("enabled host 1");
        assert_eq!(read1.disabled, None);

        let mut disabled_host = host.clone();
        disabled_host.disabled = Some(true);
        save_store(
            &path,
            &SshHostStore {
                hosts: vec![disabled_host],
                tombstones: vec![],
            },
        )
        .expect("save disabled host");

        let err = crate::ssh::projects::enabled_host(&path, "h1")
            .expect_err("should reject disabled host");
        assert_eq!(
            err.details
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str()),
            Some("hostDisabled")
        );
    }

    const TEST_KEY: &str = "fxk_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn node_install_code(error: &IpcError) -> Option<&str> {
        match &error.code {
            IpcErrorCode::Custom(code) => Some(code.as_str()),
            _ => None,
        }
    }

    fn detail<'a>(error: &'a IpcError, key: &str) -> Option<&'a serde_json::Value> {
        error.details.as_ref().and_then(|details| details.get(key))
    }

    #[test]
    fn node_install_command_never_contains_the_key() {
        let origin = validate_install_origin("https://ferryx.dev/").expect("https origin");
        let command = node_install_command(&origin);
        assert!(!command.contains(TEST_KEY));
        assert!(!command.contains("fxk_"));
        assert!(command.contains("--key-stdin --json"));
        assert!(command.contains("command -v curl >/dev/null 2>&1"));
        assert!(command.contains("wget -qO"));
        let input = node_install_stdin(TEST_KEY).expect("key stdin");
        assert_eq!(input, format!("{TEST_KEY}\n").into_bytes());
    }

    #[test]
    fn node_install_command_quotes_the_origin() {
        let origin = validate_install_origin("https://ferryx.dev").expect("https origin");
        let command = node_install_command(&origin);
        assert!(command.contains("FERRYX_ORIGIN='https://ferryx.dev' sh"));
        assert!(command.contains("curl -fsSL 'https://ferryx.dev/install.sh'"));
        let tricky = "https://ex.test/a'b";
        let quoted = node_install_command(tricky);
        assert!(quoted.contains("FERRYX_ORIGIN='https://ex.test/a'\\''b' sh"));
    }

    #[test]
    fn node_install_origin_rejects_non_https() {
        for origin in [
            "http://ferryx.dev",
            "ftp://ferryx.dev",
            "ferryx.dev",
            "https://user@ferryx.dev",
            "https://ferryx.dev/?x=1",
            "file:///etc/passwd",
        ] {
            let error = validate_install_origin(origin).expect_err(origin);
            assert_eq!(error.code, IpcErrorCode::InvalidArgument, "{origin}");
        }
        assert_eq!(
            validate_install_origin("http://127.0.0.1:8787/").unwrap(),
            "http://127.0.0.1:8787"
        );
        assert_eq!(
            validate_install_origin("http://localhost:3000").unwrap(),
            "http://localhost:3000"
        );
    }

    #[test]
    fn node_install_stdin_rejects_empty_or_multiline_keys() {
        assert!(node_install_stdin("").is_err());
        assert!(node_install_stdin("fxk_a\nrm -rf /").is_err());
    }

    #[test]
    fn node_install_report_uses_last_json_line_after_noise() {
        let stdout = b"Downloading ferryx-cli...\n{\"progress\":1}\nnot json {\n\
{\"cliPath\":\"/home/u/.local/bin/ferryx-cli\",\"version\":\"2026.1023.1\",\
\"enrolled\":{\"accountId\":\"acc_1\",\"machineRecordId\":\"m_1\",\"extra\":true},\
\"service\":{\"supervised\":false,\"backend\":\"detached\",\"reason\":\"no systemd\",\"pid\":42},\
\"relayOrigin\":\"https://relay.ferryx.dev\"}\n\n";
        let report = parse_node_install_report(stdout).expect("report");
        assert_eq!(report.cli_path, "/home/u/.local/bin/ferryx-cli");
        assert_eq!(report.version, "2026.1023.1");
        assert_eq!(
            report.enrolled,
            Some(SshNodeEnrollment {
                account_id: "acc_1".into(),
                machine_record_id: "m_1".into(),
            })
        );
        assert!(!report.service.supervised);
        assert_eq!(report.service.backend, "detached");
        assert_eq!(report.service.reason.as_deref(), Some("no systemd"));
    }

    #[test]
    fn node_install_report_without_json_is_a_typed_parse_error() {
        let error = parse_node_install_report(b"installed\nall good\n").expect_err("no json");
        assert_eq!(node_install_code(&error), Some(SSH_NODE_INSTALL_FAILED));
        assert_eq!(detail(&error, "stage").and_then(|v| v.as_str()), Some("parse"));
        let wrong_shape = parse_node_install_report(b"{\"version\":\"1\"}\n").expect_err("shape");
        assert_eq!(node_install_code(&wrong_shape), Some(SSH_NODE_INSTALL_FAILED));
    }

    #[test]
    fn node_install_failure_redacts_key_and_bounds_stderr_tail() {
        let stderr = format!("{}error: key {TEST_KEY} was rejected", "x".repeat(5000));
        let source = IpcError::new(IpcErrorCode::IoError, "SSH command failed (exit 3)")
            .with_details(serde_json::json!({
                "stage": "execution",
                "exitCode": 3,
                "stderr": stderr,
            }));
        let error = node_install_failure(source, TEST_KEY);
        assert_eq!(node_install_code(&error), Some(SSH_NODE_INSTALL_FAILED));
        assert_eq!(detail(&error, "stage").and_then(|v| v.as_str()), Some("install"));
        assert_eq!(detail(&error, "exitCode").and_then(|v| v.as_i64()), Some(3));
        let tail = detail(&error, "stderrTail").and_then(|v| v.as_str()).unwrap();
        assert!(tail.len() <= 2000);
        assert!(!tail.contains(TEST_KEY));
        assert!(tail.ends_with("error: key [REDACTED] was rejected"));
        assert!(!serde_json::to_string(&error).unwrap().contains(TEST_KEY));
    }

    #[test]
    fn node_install_failure_classifies_transport_and_download() {
        let transport = node_install_failure(
            crate::ssh::runtime::error(IpcErrorCode::IoError, "transport", "SSH operation timed out"),
            TEST_KEY,
        );
        assert_eq!(detail(&transport, "stage").and_then(|v| v.as_str()), Some("transport"));
        let download = node_install_failure(
            IpcError::new(IpcErrorCode::IoError, "failed").with_details(serde_json::json!({
                "stage": "execution",
                "exitCode": 97,
                "stderr": "curl: (22) 404",
            })),
            TEST_KEY,
        );
        assert_eq!(detail(&download, "stage").and_then(|v| v.as_str()), Some("download"));
    }
}
