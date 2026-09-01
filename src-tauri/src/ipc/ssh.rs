use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::ssh::config::parse_ssh_config;
use crate::ssh::exec::probe_argv;
use crate::ssh::worktree::{parse_worktree_porcelain, remote_add_argv, remote_list_argv, remote_remove_argv};
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

fn get_ssh_store_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
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
            IpcError::new(IpcErrorCode::IoError, format!("Failed to create ssh store dir: {}", e))
        })?;
    }
    let serialized = serde_json::to_string_pretty(store)
        .map_err(|e| IpcError::internal(format!("Failed to serialize ssh store: {}", e)))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serialized.as_bytes())
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| IpcError::new(IpcErrorCode::IoError, format!("Failed to write ssh store: {}", e)))
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn read_default_config_text() -> Option<String> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    std::fs::read_to_string(home.join(".ssh").join("config")).ok()
}

fn stderr_suggests_login_shell(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("not found")
}

/// Runs a remote git command; on Unix non-login shells the profile PATH is
/// skipped, so a "not found" failure is retried once through `sh -lc`.
/// Windows hosts (cmd/PowerShell) report "not recognized" and are never retried.
fn run_remote_git_command(host: &SshHost, remote_command: &str) -> std::io::Result<std::process::Output> {
    let output = std::process::Command::new("ssh")
        .args([host.target(), remote_command.to_string()])
        .output()?;
    if output.status.success() || !stderr_suggests_login_shell(&String::from_utf8_lossy(&output.stderr))
    {
        return Ok(output);
    }
    std::process::Command::new("ssh")
        .args([host.target(), "sh".to_string(), "-lc".to_string(), remote_command.to_string()])
        .output()
}

#[tauri::command]
pub async fn cmd_ssh_list_hosts<R: Runtime>(app: AppHandle<R>) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || Ok(load_store(&path).hosts)).await
}

#[tauri::command]
pub async fn cmd_ssh_import_config<R: Runtime>(
    app: AppHandle<R>,
    config_text: Option<String>,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || {
        let text = match config_text {
            Some(text) if !text.trim().is_empty() => text,
            _ => read_default_config_text().ok_or_else(|| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    "Failed to read ~/.ssh/config".to_string(),
                )
            })?,
        };
        let mut store = load_store(&path);
        let parsed = parse_ssh_config(&text);
        let existing_keys: Vec<String> = store.hosts.iter().map(|host| host.key()).collect();
        let mut tombstones = store.tombstones.clone();
        tombstones.extend(existing_keys);
        let imported = crate::ssh::config::import_aliases(&parsed, &tombstones);
        for host in imported {
            if !store.hosts.iter().any(|existing| existing.key() == host.key()) {
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
        if let Some(slot) = store.hosts.iter_mut().find(|existing| existing.id == host.id) {
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
pub async fn cmd_ssh_delete_host<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Vec<SshHost>, IpcError> {
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
    run_blocking(move || {
        let output = std::process::Command::new(&probe_argv(&host)[0])
            .args(&probe_argv(&host)[1..])
            .output();
        let (reachable, last_error) = match output {
            Ok(result) if result.status.success() => (true, None),
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                let message = stderr.lines().last().unwrap_or("ssh failed").to_string();
                (false, Some(message))
            }
            Err(error) => (false, Some(format!("failed to spawn ssh: {error}"))),
        };
        Ok(SshTargetSummary {
            host,
            reachable,
            last_error,
            checked_at: now_millis(),
        })
    })
    .await
}

#[tauri::command]
pub async fn cmd_ssh_list_remote_worktrees(
    host: SshHost,
) -> Result<Vec<crate::ssh::worktree::RemoteWorktree>, IpcError> {
    run_blocking(move || {
        let argv = remote_list_argv(&host);
        let output = run_remote_git_command(&host, argv.last().map(String::as_str).unwrap_or_default())
            .map_err(|error| IpcError::internal(format!("failed to spawn ssh: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(IpcError::internal(format!("remote worktree list failed: {stderr}")));
        }
        Ok(parse_worktree_porcelain(&String::from_utf8_lossy(&output.stdout)))
    })
    .await
}

#[tauri::command]
pub async fn cmd_ssh_create_remote_worktree(
    host: SshHost,
    path: String,
    ws_id: String,
    slug: String,
    base_ref: Option<String>,
) -> Result<(), IpcError> {
    run_blocking(move || {
        let argv = remote_add_argv(&host, &path, &ws_id, &slug, base_ref.as_deref())
            .map_err(IpcError::internal)?;
        let output = run_remote_git_command(&host, argv.last().map(String::as_str).unwrap_or_default())
            .map_err(|error| IpcError::internal(format!("failed to spawn ssh: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(IpcError::internal(format!("remote worktree create failed: {stderr}")));
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn cmd_ssh_delete_remote_worktree(host: SshHost, path: String) -> Result<(), IpcError> {
    run_blocking(move || {
        let argv = remote_remove_argv(&host, &path);
        let output = run_remote_git_command(&host, argv.last().map(String::as_str).unwrap_or_default())
            .map_err(|error| IpcError::internal(format!("failed to spawn ssh: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(IpcError::internal(format!("remote worktree remove failed: {stderr}")));
        }
        Ok(())
    })
    .await
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
                repo_root: None,
                remote_continuity: crate::ssh::RemoteContinuity::Auto,
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
    fn red_stderr_suggests_login_shell_retry() {
        // Unix shells phrase missing commands as "not found".
        assert!(stderr_suggests_login_shell(
            "bash: line 1: git: command not found"
        ));
        assert!(stderr_suggests_login_shell("sh: 1: git: not found"));
        assert!(stderr_suggests_login_shell("zsh:1: command not found: git"));
        // Windows shells must NOT trigger the Unix login-shell retry.
        assert!(!stderr_suggests_login_shell(
            "'git' is not recognized as an internal or external command"
        ));
        assert!(!stderr_suggests_login_shell(
            "The term 'git' is not recognized as the name of a cmdlet"
        ));
        assert!(!stderr_suggests_login_shell(""));
    }
}
