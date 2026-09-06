use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::ssh::config::parse_ssh_config;
use crate::ssh::SshHost;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTargetSummary {
    pub host: SshHost,
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub checked_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshHostStore {
    #[serde(default)]
    pub hosts: Vec<SshHost>,
    #[serde(default)]
    pub tombstones: Vec<String>,
}

pub(crate) fn get_ssh_store_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
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

fn load_store(path: &PathBuf) -> SshHostStore {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => SshHostStore::default(),
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
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serialized.as_bytes())
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to write ssh store: {}", e),
            )
        })
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[tauri::command]
pub async fn cmd_ssh_list_hosts<R: Runtime>(app: AppHandle<R>) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || Ok(load_store(&path).hosts)).await
}

#[tauri::command]
pub async fn cmd_ssh_import_config<R: Runtime>(
    app: AppHandle<R>,
    config_text: String,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || {
        let mut store = load_store(&path);
        let parsed = parse_ssh_config(&config_text);
        let existing_keys: Vec<String> = store.hosts.iter().map(|host| host.key()).collect();
        let mut tombstones = store.tombstones.clone();
        tombstones.extend(existing_keys);
        let imported = crate::ssh::config::import_aliases(&parsed, &tombstones);
        for host in imported {
            if !store
                .hosts
                .iter()
                .any(|existing| existing.key() == host.key())
            {
                store.hosts.push(host);
            }
        }
        save_store(&path, &store)?;
        Ok(store.hosts)
    })
    .await
}

#[tauri::command]
pub async fn cmd_ssh_update_host<R: Runtime>(
    app: AppHandle<R>,
    host: SshHost,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || {
        let mut store = load_store(&path);
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

#[tauri::command]
pub async fn cmd_ssh_delete_host<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || {
        let mut store = load_store(&path);
        if let Some(position) = store.hosts.iter().position(|host| host.id == id) {
            let removed = store.hosts.remove(position);
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

#[tauri::command]
pub async fn cmd_ssh_test_connection(host: SshHost) -> Result<SshTargetSummary, IpcError> {
    let plan = crate::ssh::direct::ssh_plan(&host, "true".into(), false)?;
    let result = crate::ssh::direct::bounded_output(&plan, std::time::Duration::from_secs(8)).await;
    Ok(SshTargetSummary {
        host,
        reachable: result.is_ok(),
        last_error: result.err().map(|e| e.message),
        checked_at: now_millis(),
    })
}

#[tauri::command]
pub async fn cmd_ssh_list_remote_worktrees(
    host: SshHost,
) -> Result<Vec<crate::ssh::worktree::RemoteWorktree>, IpcError> {
    let _ = host;
    Err(crate::ssh::projects::unsupported())
}

#[tauri::command]
pub async fn cmd_ssh_create_remote_worktree(
    host: SshHost,
    path: String,
    ws_id: String,
    slug: String,
    base_ref: Option<String>,
) -> Result<(), IpcError> {
    let _ = (host, path, ws_id, slug, base_ref);
    Err(crate::ssh::projects::unsupported())
}

#[tauri::command]
pub async fn cmd_ssh_delete_remote_worktree(host: SshHost, path: String) -> Result<(), IpcError> {
    let _ = (host, path);
    Err(crate::ssh::projects::unsupported())
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
}
