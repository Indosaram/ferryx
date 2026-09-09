use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::ssh::config::parse_ssh_config;
use crate::ssh::SshHost;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTargetSummary {
    pub host: SshHost,
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub environment: Option<crate::ssh::runtime::RemoteEnvironment>,
    pub diagnostic: Option<IpcError>,
    pub checked_at: u64,
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

pub fn resolve_home_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .home_dir()
        .ok()
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
        })
}

pub fn system_ssh_config_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    resolve_home_dir(app).map(|home| home.join(".ssh").join("config"))
}

pub(crate) fn expand_tilde_path(raw: &str, home: Option<&Path>) -> PathBuf {
    if raw == "~" {
        home.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(raw))
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
    run_blocking(move || import_config_into_store(&path, &config_text)).await
}

fn import_config_into_store(path: &PathBuf, config_text: &str) -> Result<Vec<SshHost>, IpcError> {
    let mut store = load_store(path);
    let parsed = parse_ssh_config(config_text);
    let already_stored: Vec<String> = store.hosts.iter().map(|host| host.key()).collect();
    let imported = crate::ssh::config::import_aliases(&parsed, &already_stored);
    for host in imported {
        let key = host.key();
        store.tombstones.retain(|tombstone| tombstone != &key);
        store.hosts.push(host);
    }
    save_store(path, &store)?;
    Ok(store.hosts)
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
    let result = crate::ssh::runtime::detect(&host).await;
    let reachable = result.is_ok();
    let (environment, diagnostic) = match result {
        Ok(environment) => (Some(environment), None),
        Err(error) => (None, Some(error)),
    };
    Ok(SshTargetSummary {
        host,
        reachable,
        last_error: diagnostic.as_ref().map(|e| e.message.clone()),
        environment,
        diagnostic,
        checked_at: now_millis(),
    })
}

#[tauri::command]
pub async fn cmd_ssh_prepare_integration(host: SshHost) -> Result<(), IpcError> {
    crate::ssh::direct::ensure_remote_extension_installed(&host).await
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshClipboardImagePaste {
    pub remote_path: String,
    pub byte_length: usize,
}

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

#[tauri::command]
pub async fn cmd_ssh_list_remote_worktrees<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
) -> Result<Vec<crate::ssh::worktree::RemoteWorktree>, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let id = workspace_id.clone();
    let (project, host) =
        run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
    let environment = crate::ssh::runtime::detect(&host).await?;
    crate::ssh::worktree::list_remote(&host, &environment, &project.repo_root).await
}

#[tauri::command]
pub async fn cmd_ssh_create_remote_worktree<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    slug: String,
    base_ref: Option<String>,
) -> Result<crate::ssh::worktree::RemoteWorktree, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let id = workspace_id.clone();
    let (project, host) =
        run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
    let environment = crate::ssh::runtime::detect(&host).await?;
    let ws_segment = crate::ssh::worktree::derive_ws_segment(&workspace_id)?;
    let wt_path = crate::ssh::worktree::remote_worktree_path(
        environment.platform,
        &project.repo_root,
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
            wt.path == wt_path
                || wt.path.replace('\\', "/").trim_end_matches('/') == norm_wt_path
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

#[tauri::command]
pub async fn cmd_ssh_delete_remote_worktree<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    path: String,
) -> Result<(), IpcError> {
    let store = get_ssh_store_path(&app)?;
    let id = workspace_id.clone();
    let (project, host) =
        run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
    let environment = crate::ssh::runtime::detect(&host).await?;
    crate::ssh::worktree::validate_path_inside_root(
        environment.platform,
        &project.repo_root,
        &path,
    )?;
    crate::ssh::worktree::remove_remote(&host, &environment, &project.repo_root, &path).await?;
    let slug = path
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .last()
        .and_then(|name| name.strip_prefix("wt-"))
        .unwrap_or("")
        .to_string();
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
    fn explicit_import_restores_a_previously_deleted_config_host() {
        // Given a deleted config host and an unrelated deletion.
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

        // When the user explicitly imports that host again.
        let hosts = import_config_into_store(
            &path,
            "Host dev-box\n  HostName dev.example\n  User dev\n",
        )
        .expect("import config");

        // Then it is returned and persisted, without clearing unrelated deletions.
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].label, "dev-box");
        let saved = load_store(&path);
        assert_eq!(saved.hosts, hosts);
        assert_eq!(saved.tombstones, vec!["other.example:22"]);
    }

    #[test]
    fn repeated_import_preserves_existing_hosts_without_duplicates() {
        // Given a host already saved with user edits.
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

        // When the same config is explicitly imported.
        let imported = import_config_into_store(&path, config).expect("import config");

        // Then the existing record is neither duplicated nor overwritten.
        assert_eq!(imported, hosts);
        assert_eq!(load_store(&path).hosts, hosts);
    }

    #[test]
    fn explicit_config_file_is_read_and_parsed_from_its_own_path() {
        // Given an SSH config kept outside the default ~/.ssh/config location.
        let dir = tempfile::tempdir().expect("temporary dir");
        let path = dir.path().join("work-ssh-config");
        std::fs::write(&path, "Host work-box\n  HostName work.example\n  User dev\n")
            .expect("write config");

        // When that file is read.
        let result = read_ssh_config_file(&path).expect("read config");

        // Then its hosts are returned and the reported path is that file.
        assert!(result.exists);
        assert_eq!(result.path, path.to_string_lossy());
        assert_eq!(result.hosts.len(), 1);
        assert_eq!(result.hosts[0].label, "work-box");
        assert_eq!(result.hosts[0].hostname, "work.example");
        assert_eq!(result.hosts[0].username, Some("dev".into()));
    }

    #[test]
    fn missing_config_file_reports_absence_instead_of_failing() {
        // Given a path that holds no file.
        let dir = tempfile::tempdir().expect("temporary dir");
        let path = dir.path().join("absent-config");

        // When it is read.
        let result = read_ssh_config_file(&path).expect("read config");

        // Then absence is reported without hosts.
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
}
