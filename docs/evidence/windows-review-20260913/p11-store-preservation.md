# P11 SSH store preservation - 2026-09-13

Task st_01a099d0; FSSH-06; C002. Local macOS behavioral repair complete. Windows deny-read/share-delete handle runtime remains pending; no Windows acceptance claim.

## Change and inspected mechanism

Only src-tauri/src/ipc/ssh.rs and this report are delivered. UI import/update/delete invoke registered Tauri commands; run_blocking delegates to the actual store mutation code. Previously load_store converted any read/whole-store parse failure into an empty store, allowing save_store to overwrite inventory. Loading now returns Result; only NotFound defaults, JSON failures return ParseError, other read failures return IoError. List/import/update/delete propagate failures before saving. save_store and resilient per-entry parsing are unchanged. No concurrent-writer redesign.

Read repair-packets.md P11, audit-filesystem-ssh.md, root/backend/IPC instructions, all selected embedded tests, config parser/import implementation, IPC errors/blocking helper, UI callers, Cargo manifest and Ghostty build prerequisites. Tests use in-memory data or owned tempfile paths, no ambient SSH config, profile lookup, SSH, GUI, daemon, delays or child processes. Build prerequisites invoke compiler/Zig and read-only Git revision inspection; no project helper executable is required. Zig 0.16.0 and Ghostty revision 6a508fd5e34c7e222c052a6d00bb3891ff3feace were inspected. Rust is direct installed 1.98.1, not ambient 1.92.

## Actual RED/GREEN

Lead ran monitor bash56 RED and bash58 GREEN; child independently read complete logs and exit/environment receipts below. Identical command and environment verified programmatically:

`cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::ssh:: -- --nocapture`

- RED: exit 101; 12 passed, 1 failed, 1039 filtered. import_preserves_corrupt_store_bytes failed the joint typed-error/exact-byte assertion: actual success and replacement inventory versus expected ParseError and original corrupt bytes.
- GREEN: exit 0; 13 passed, 0 failed, 1039 filtered. Same real import mutator, exact-byte assertion, missing-file creation control and resilient-entry/duplicate tests passed. No tests skipped or assertions weakened. Three existing loader calls only gained expect for Result.
- Both builds report 17 pre-existing warnings outside the changed file; full output retained below. No suppression or unrelated edits.
- Fresh LSP diagnostics after production edit: tool lsp_diagnostics, absolute path src-tauri/src/ipc/ssh.rs, severity all: "No diagnostics found". git diff --check -- src-tauri/src/ipc/ssh.rs: exit 0, no output.

## Source and regression binding

Original clean source SHA256: 5fbfbe4c3803fd0a272d8ab98552a6414642d73072d2c71e83c1d8dc4263e4af.
RED source: e56f26243a465a527d5066f405ca3467b683d97006a96f99b8b7792c243dff9b.
GREEN/current source: 6bb4ebd9b62715254ffdec6dcb61489be86ef2417efa51b9c6087bff809de433.

Reverse-applied only the recorded production changes and the three Result expect adaptations IN MEMORY to current source; reconstructed full RED source SHA256 equals the pre-run RED receipt. Neither working source nor tests were edited for this comparison. Both new regression functions are byte-identical between those bound versions; combined block SHA256: 138d1babce5fd6acf634a47bf11ea1400ba9fcaa3dcc0a22c4fb3ad1f49edd14. Full GREEN source and unchanged regression block are retained below. Runtime binary path is recorded by Cargo in each log; binary hash at run time was not captured, and shared target is now used by other packets, so no later binary hash is misrepresented as historical provenance.

## Isolation and cleanup ownership

P11 root: /private/tmp/ferryx-p11-st_01a099d0.OHtDkm. Shared target: /private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target, lead-assigned and serialized; must NOT be deleted by P11. Own HOME/profile/appdata/data/runtime/session/temp directories and Cargo metadata; registry borrowed through P32 registry symlink read-only under sandbox-exec. No duplicate cache copying occurred. Sandbox denies network and source/ambient registry/toolchain/shared Cargo writes. Default debug/incremental settings retained. Full runner and environment below. No commits/branches/worktrees/refs/pushes or runtime service actions. Uncommitted shared-tree changes remain subject to concurrent owners.

## Full artifact: green-source.rs

SHA256: 6bb4ebd9b62715254ffdec6dcb61489be86ef2417efa51b9c6087bff809de433; UTF-8 bytes: 25910.

```rust
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
    #[serde(default, deserialize_with = "deserialize_resilient_hosts")]
    pub hosts: Vec<SshHost>,
    #[serde(default)]
    pub tombstones: Vec<String>,
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
                tracing::warn!("Skipping unparseable SSH host at index {index} (id: {id_display}): {e}");
            }
        }
    }
    Ok(hosts)
}

pub(crate) fn get_ssh_store_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, IpcError> {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR") { return Ok(PathBuf::from(dir).join("ssh_hosts.json")); }
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
    run_blocking(move || Ok(load_store(&path)?.hosts)).await
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
    let mut store = load_store(path)?;
    let parsed = parse_ssh_config(config_text);
    let already_stored: Vec<String> = store.hosts.iter().map(|host| host.key()).collect();
    let imported = crate::ssh::config::import_aliases(&parsed, &already_stored);
    for host in imported {
        let key = host.key();
        store.tombstones.retain(|tombstone| tombstone != &key);
        if let Some(position) = store.hosts.iter().position(|existing| existing.id == host.id) {
            store.hosts[position] = host;
        } else {
            store.hosts.push(host);
        }
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

#[tauri::command]
pub async fn cmd_ssh_delete_host<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<Vec<SshHost>, IpcError> {
    let path = get_ssh_store_path(&app)?;
    run_blocking(move || {
        let mut store = load_store(&path)?;
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

/// Explicit user-selected artifact installation; no build or download is performed.
#[tauri::command]
pub async fn cmd_ssh_install_project_helper<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: String,
    local_binary: PathBuf,
) -> Result<crate::ssh::helper_setup::HelperLocation, IpcError> {
    let store = get_ssh_store_path(&app)?;
    let (_, host) = super::run_blocking(move || crate::ssh::projects::resolve(&store, &workspace_id)).await?;
    let environment = crate::ssh::runtime::detect(&host).await?;
    let location = crate::ssh::helper_setup::default_location(&host, &environment)?;
    crate::ssh::helper_setup::install(&host, &environment, &location, &local_binary).await?;
    Ok(location)
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
            (result.as_ref().err().map(|error| error.code), saved.as_slice()),
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

        let hosts = import_config_into_store(
            &path,
            "Host dev-box\n  HostName dev.example\n  User dev\n",
        )
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
        std::fs::write(&path, "Host work-box\n  HostName work.example\n  User dev\n")
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
}
```

## Full artifact: unchanged-regressions.rs

SHA256: 138d1babce5fd6acf634a47bf11ea1400ba9fcaa3dcc0a22c4fb3ad1f49edd14; UTF-8 bytes: 1450.

```rust
    #[test]
    fn import_preserves_corrupt_store_bytes() {
        let dir = tempfile::tempdir().expect("temporary store");
        let path = dir.path().join("ssh_hosts.json");
        let original = b"{\"hosts\": [\n  {\"id\": \"saved-host\"},";
        std::fs::write(&path, original).expect("seed corrupt store");

        let result = import_config_into_store(&path, "Host new-box\n  HostName new.example\n");
        let saved = std::fs::read(&path).expect("read inventory after import");
        assert_eq!(
            (result.as_ref().err().map(|error| error.code), saved.as_slice()),
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

```

## Full artifact: run.mjs

SHA256: 620a3f010bdc8fc8a795c5ad00046178a37840413f4fe553c372a8d5047abbd7; UTF-8 bytes: 3324.

```javascript
import { mkdirSync, existsSync, symlinkSync, writeFileSync, readFileSync, createWriteStream } from 'node:fs';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
const root = '/private/tmp/ferryx-p11-st_01a099d0.OHtDkm';
const repo = '/Users/indo/code/project/orca-lite';
const shared = '/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW';
const policy = `(version 1)(allow default)(deny network*)(deny file-write* (subpath "${repo}") (subpath "/Users/indo/.cargo") (subpath "/Users/indo/.rustup") (subpath "${shared}/cargo"))`;
const phase = process.argv[2];
if (!['red', 'green'].includes(phase)) throw new Error('expected red or green');
for (const name of ['home', 'profile', 'appdata', 'localappdata', 'data', 'runtime', 'sessions', 'temp', 'cargo', 'cache', 'config']) mkdirSync(`${root}/${name}`, { recursive: true });
const toolchain = '/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin';
const env = {
 PATH: `${toolchain}:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin`,
 HOME: `${root}/home`, USERPROFILE: `${root}/profile`, APPDATA: `${root}/appdata`, LOCALAPPDATA: `${root}/localappdata`,
 FERRYX_DATA_DIR: `${root}/data`, FERRYX_RUNTIME_DIR: `${root}/runtime`, FERRYX_SESSION_DIR: `${root}/sessions`,
 TMPDIR: `${root}/temp`, TMP: `${root}/temp`, TEMP: `${root}/temp`, XDG_CACHE_HOME: `${root}/cache`, XDG_CONFIG_HOME: `${root}/config`, XDG_DATA_HOME: `${root}/data`,
 CARGO_HOME: `${root}/cargo`, CARGO_TARGET_DIR: `${shared}/target`, CARGO_NET_OFFLINE: 'true', CARGO_BUILD_JOBS: '8', CARGO_TERM_COLOR: 'never', RUSTC_WRAPPER: '',
 RUSTC: `${toolchain}/rustc`, RUSTDOC: `${toolchain}/rustdoc`, ZIG: '/opt/homebrew/bin/zig', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LANG: 'en_US.UTF-8',
};
async function run(command, args, log) {
 const child = spawn(command, args, { cwd: repo, env, stdio: ['ignore', 'pipe', 'pipe'] });
 const output = createWriteStream(log, { flags: 'wx' });
 child.stdout.on('data', b => { output.write(b); process.stdout.write(b); });
 child.stderr.on('data', b => { output.write(b); process.stderr.write(b); });
 const code = await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', resolve); });
 await new Promise(resolve => output.end(resolve));
 return code;
}
// Borrow only the registry; local Cargo locks/metadata remain owned by P11.
// sandbox-exec makes the borrowed registry (including its ambient symlink target) read-only.
if (!existsSync(`${root}/cargo/registry`)) symlinkSync(`${shared}/cargo/registry`, `${root}/cargo/registry`);
const args = ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', 'ipc::ssh::', '--', '--nocapture'];
const sourceHash = createHash('sha256').update(readFileSync(`${repo}/src-tauri/src/ipc/ssh.rs`)).digest('hex');
writeFileSync(`${root}/${phase}-environment.json`, JSON.stringify({ root, repo, shared, env, policy, command: ['cargo', ...args], sourceHash, startedAt: new Date().toISOString() }, null, 2), { flag: 'wx' });
const code = await run('/usr/bin/sandbox-exec', ['-p', policy, 'cargo', ...args], `${root}/${phase}.log`);
writeFileSync(`${root}/${phase}-exit.json`, JSON.stringify({ code, endedAt: new Date().toISOString() }) + '\n', { flag: 'wx' });
console.log(`P11_${phase.toUpperCase()}_EXIT=${code}`);
process.exitCode = code;
```

## Full artifact: red-environment.json

SHA256: cae8d285811d4f279f7954fb29a98b3b7087e8353660b06ab3ab78feb5261905; UTF-8 bytes: 2352.

```json
{
  "root": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm",
  "repo": "/Users/indo/code/project/orca-lite",
  "shared": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW",
  "env": {
    "PATH": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    "HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/home",
    "USERPROFILE": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/profile",
    "APPDATA": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/appdata",
    "LOCALAPPDATA": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/localappdata",
    "FERRYX_DATA_DIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/data",
    "FERRYX_RUNTIME_DIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/runtime",
    "FERRYX_SESSION_DIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/sessions",
    "TMPDIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/temp",
    "TMP": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/temp",
    "TEMP": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/temp",
    "XDG_CACHE_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cache",
    "XDG_CONFIG_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/config",
    "XDG_DATA_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/data",
    "CARGO_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cargo",
    "CARGO_TARGET_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target",
    "CARGO_NET_OFFLINE": "true",
    "CARGO_BUILD_JOBS": "8",
    "CARGO_TERM_COLOR": "never",
    "RUSTC_WRAPPER": "",
    "RUSTC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustc",
    "RUSTDOC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustdoc",
    "ZIG": "/opt/homebrew/bin/zig",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "LANG": "en_US.UTF-8"
  },
  "policy": "(version 1)(allow default)(deny network*)(deny file-write* (subpath \"/Users/indo/code/project/orca-lite\") (subpath \"/Users/indo/.cargo\") (subpath \"/Users/indo/.rustup\") (subpath \"/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo\"))",
  "command": [
    "cargo",
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--lib",
    "ipc::ssh::",
    "--",
    "--nocapture"
  ],
  "sourceHash": "e56f26243a465a527d5066f405ca3467b683d97006a96f99b8b7792c243dff9b",
  "startedAt": "2026-09-13T08:14:22.790Z"
}
```

## Full artifact: red-exit.json

SHA256: 19d3edc36e070b96d01fd52f0df345d983155e286beb2caddbc1c0f7d6521803; UTF-8 bytes: 50.

```json
{"code":101,"endedAt":"2026-09-13T08:15:41.976Z"}
```

## Full artifact: red.log

SHA256: fa0223df2d0199416dc3328cd14eab09eea90e70597f260060b5493280de2f44; UTF-8 bytes: 20607.

```text
   Compiling proc-macro2 v1.0.107
   Compiling quote v1.0.47
   Compiling unicode-ident v1.0.24
   Compiling cfg-if v1.0.4
   Compiling libc v0.2.189
   Compiling serde_core v1.0.229
   Compiling shlex v2.0.1
   Compiling find-msvc-tools v0.1.11
   Compiling cc v1.4.3
   Compiling itoa v1.0.18
   Compiling parking_lot_core v0.9.12
   Compiling memchr v2.8.3
   Compiling scopeguard v1.2.0
   Compiling log v0.4.33
   Compiling lock_api v0.4.14
   Compiling litemap v0.8.3
   Compiling writeable v0.6.4
   Compiling bytes v1.12.1
   Compiling utf8_iter v1.0.4
   Compiling siphasher v1.0.3
   Compiling thiserror v2.0.20
   Compiling icu_properties_data v2.3.0
   Compiling icu_normalizer_data v2.3.0
   Compiling phf_shared v0.13.1
   Compiling zmij v1.0.23
   Compiling objc2-encode v4.1.0
   Compiling autocfg v1.5.1
   Compiling syn v3.0.3
   Compiling syn v2.0.119
   Compiling fastrand v2.5.0
   Compiling phf_generator v0.13.1
   Compiling objc2-exception-helper v0.1.1
   Compiling stable_deref_trait v1.2.1
   Compiling http v1.5.0
   Compiling getrandom v0.4.3
   Compiling objc2 v0.6.4
   Compiling bitflags v2.13.1
   Compiling smallvec v1.15.2
   Compiling serde v1.0.229
   Compiling equivalent v1.0.2
   Compiling time-core v0.1.9
   Compiling num-conv v0.2.2
   Compiling powerfmt v0.2.0
   Compiling synstructure v0.13.2
   Compiling winnow v1.0.4
   Compiling once_cell v1.21.4
   Compiling toml_parser v1.1.3+spec-1.1.0
   Compiling toml_writer v1.1.2+spec-1.1.0
   Compiling base64 v0.22.1
   Compiling serde_derive v1.0.229
   Compiling zerovec-derive v0.11.5
   Compiling displaydoc v0.2.7
   Compiling zerofrom-derive v0.1.7
   Compiling yoke-derive v0.8.2
   Compiling thiserror-impl v2.0.20
   Compiling phf_macros v0.13.1
   Compiling phf_codegen v0.13.1
   Compiling serde_json v1.0.151
   Compiling thiserror v1.0.69
   Compiling zerofrom v0.1.8
   Compiling strsim v0.11.1
   Compiling typeid v1.0.3
   Compiling ident_case v1.0.1
   Compiling darling_core v0.23.0
   Compiling thiserror-impl v1.0.69
   Compiling semver v1.0.28
   Compiling erased-serde v0.4.10
   Compiling byteorder v1.5.0
   Compiling yoke v0.8.3
   Compiling block2 v0.6.2
   Compiling dispatch2 v0.3.1
   Compiling objc2-core-foundation v0.3.2
   Compiling darling_macro v0.23.0
   Compiling aho-corasick v1.1.5
   Compiling unic-char-range v0.9.0
   Compiling unic-common v0.9.0
   Compiling new_debug_unreachable v1.0.6
   Compiling regex-syntax v0.8.11
   Compiling objc2-foundation v0.3.2
   Compiling fnv v1.0.7
   Compiling unic-ucd-version v0.9.0
   Compiling unic-char-property v0.9.0
   Compiling darling v0.23.0
   Compiling phf v0.13.1
   Compiling regex-automata v0.4.18
   Compiling zerovec v0.11.8
   Compiling zerotrie v0.2.5
   Compiling string_cache_codegen v0.6.1
   Compiling alloc-no-stdlib v2.0.4
   Compiling anyhow v1.0.104
   Compiling precomputed-hash v0.1.1
   Compiling alloc-stdlib v0.2.4
   Compiling web_atoms v0.2.6
   Compiling regex v1.13.1
   Compiling serde_with_macros v3.22.0
   Compiling parking_lot v0.12.5
   Compiling unic-ucd-ident v0.9.0
   Compiling serde_spanned v1.1.1
   Compiling quick-xml v0.41.0
   Compiling same-file v1.0.6
   Compiling string_cache v0.9.0
   Compiling walkdir v2.5.0
   Compiling brotli-decompressor v5.0.3
   Compiling dtoa v1.0.11
   Compiling dunce v1.0.5
   Compiling ctor-proc-macro v0.0.7
   Compiling percent-encoding v2.3.2
   Compiling ctor v0.8.0
   Compiling tinystr v0.8.4
   Compiling potential_utf v0.1.6
   Compiling icu_collections v2.3.0
   Compiling icu_locale_core v2.3.0
   Compiling form_urlencoded v1.2.2
   Compiling brotli v8.0.4
   Compiling dtoa-short v0.3.5
   Compiling uuid v1.26.0
   Compiling icu_provider v2.3.0
   Compiling tendril v0.5.1
   Compiling icu_normalizer v2.3.0
   Compiling icu_properties v2.3.0
   Compiling selectors v0.36.1
   Compiling cssparser-macros v0.6.1
   Compiling derive_more-impl v2.1.1
   Compiling toml_datetime v1.1.1+spec-1.1.0
   Compiling indexmap v1.9.3
   Compiling idna_adapter v1.2.2
   Compiling version_check v0.9.5
   Compiling glob v0.3.4
   Compiling camino v1.2.5
   Compiling idna v1.1.0
   Compiling derive_more v2.1.1
   Compiling url v2.5.8
   Compiling markup5ever v0.38.0
   Compiling toml v1.1.4+spec-1.1.0
   Compiling cssparser v0.36.0
   Compiling swift-rs v1.0.8
   Compiling bytemuck_derive v1.12.0
   Compiling serde_derive_internals v0.29.1
   Compiling servo_arc v0.4.3
   Compiling schemars v0.8.22
   Compiling hashbrown v0.17.1
   Compiling bit-vec v0.8.0
   Compiling deranged v0.5.8
   Compiling rustc-hash v2.1.3
   Compiling hashbrown v0.12.3
   Compiling indexmap v2.14.0
   Compiling schemars_derive v0.8.22
   Compiling bit-set v0.8.0
   Compiling bytemuck v1.25.2
   Compiling time v0.3.55
   Compiling html5ever v0.38.0
   Compiling cfb v0.7.3
   Compiling jsonptr v0.6.3
   Compiling cargo-platform v0.1.9
   Compiling base64 v0.21.7
   Compiling bitflags v1.3.2
   Compiling foldhash v0.2.0
   Compiling pin-project-lite v0.2.17
   Compiling dyn-clone v1.0.20
   Compiling cargo_metadata v0.19.2
   Compiling dom_query v0.27.0
   Compiling serde-untagged v0.1.9
   Compiling json-patch v3.0.1
   Compiling plist v1.10.0
   Compiling infer v0.19.0
   Compiling urlpattern v0.3.0
   Compiling serde_with v3.22.0
   Compiling errno v0.3.14
   Compiling rustc_version v0.4.1
   Compiling option-ext v0.2.0
   Compiling libm v0.2.16
   Compiling generic-array v0.14.7
   Compiling num-traits v0.2.19
   Compiling signal-hook-registry v1.4.8
   Compiling tokio-macros v2.7.2
   Compiling mio v1.2.2
   Compiling socket2 v0.6.5
   Compiling typenum v1.20.1
   Compiling tokio v1.53.1
   Compiling core-foundation-sys v0.8.7
   Compiling arrayvec v0.7.8
   Compiling raw-window-handle v0.6.2
   Compiling winnow v0.7.15
   Compiling tauri-utils v2.9.3
   Compiling toml_datetime v0.7.5+spec-1.1.0
   Compiling futures-core v0.3.34
   Compiling zerocopy v0.8.56
   Compiling toml v0.9.12+spec-1.1.0
   Compiling crypto-common v0.1.7
   Compiling block-buffer v0.10.4
   Compiling embed-resource v3.0.11
   Compiling dirs-sys v0.5.0
   Compiling zerocopy-derive v0.8.56
   Compiling heck v0.5.0
   Compiling cfg_aliases v0.2.2
   Compiling tauri-winres v0.3.6
   Compiling dirs v6.0.0
   Compiling cargo_toml v0.22.3
   Compiling digest v0.10.7
   Compiling objc2-app-kit v0.3.2
   Compiling getrandom v0.2.17
   Compiling crc32fast v1.5.0
   Compiling zeroize v1.9.0
   Compiling simd-adler32 v0.3.10
   Compiling time-macros v0.2.32
   Compiling adler2 v2.0.1
   Compiling lazy_static v1.5.0
   Compiling miniz_oxide v0.8.9
   Compiling tracing-core v0.1.36
   Compiling ring v0.17.14
   Compiling getrandom v0.3.4
   Compiling flate2 v1.1.9
   Compiling tauri-plugin v2.6.3
   Compiling tauri-build v2.6.3
   Compiling fdeflate v0.3.7
   Compiling rustls-pki-types v1.15.1
   Compiling core-foundation v0.10.1
   Compiling symphonia-core v0.5.5
   Compiling dpi v0.1.2
   Compiling tracing-attributes v0.1.31
   Compiling foreign-types-macros v0.2.4
   Compiling crossbeam-utils v0.8.22
   Compiling foreign-types-shared v0.3.1
   Compiling subtle v2.6.1
   Compiling untrusted v0.9.0
   Compiling foreign-types v0.5.0
   Compiling tracing v0.1.44
   Compiling cookie v0.18.2
   Compiling http-body v1.1.0
   Compiling rustls v0.23.43
   Compiling httparse v1.10.1
   Compiling futures-sink v0.3.34
   Compiling rustls-webpki v0.103.15
   Compiling futures-macro v0.3.34
   Compiling tauri v2.11.5
   Compiling encoding_rs v0.8.35
   Compiling slab v0.4.12
   Compiling tower-service v0.3.3
   Compiling futures-task v0.3.34
   Compiling futures-util v0.3.34
   Compiling symphonia-metadata v0.5.5
   Compiling crossbeam-channel v0.5.16
   Compiling core-graphics-types v0.2.0
   Compiling objc2-web-kit v0.3.2
   Compiling png v0.17.16
   Compiling cpufeatures v0.2.17
   Compiling tauri-runtime v2.11.3
   Compiling mime v0.3.17
   Compiling wry v0.55.1
   Compiling ico v0.5.0
   Compiling sha2 v0.10.9
   Compiling core-graphics v0.25.0
   Compiling png v0.18.1
   Compiling ppv-lite86 v0.2.21
   Compiling tauri-runtime-wry v2.11.4
   Compiling tower-layer v0.3.3
   Compiling rustc-hash v1.1.0
   Compiling unicode-segmentation v1.13.3
   Compiling try-lock v0.2.5
   Compiling pxfm v0.1.30
   Compiling want v0.3.1
   Compiling naga-types v30.0.1
   Compiling keyboard-types v0.7.0
   Compiling tao v0.35.3
   Compiling tauri-codegen v2.6.3
   Compiling webpki-roots v1.0.9
   Compiling naga v30.0.1
   Compiling sync_wrapper v1.0.2
   Compiling futures-channel v0.3.34
   Compiling serialize-to-javascript-impl v0.1.2
   Compiling bit-vec v0.9.1
   Compiling httpdate v1.0.3
   Compiling moxcms v0.8.1
   Compiling byteorder-lite v0.1.0
   Compiling objc-sys v0.3.5
   Compiling atomic-waker v1.1.2
   Compiling unicode-width v0.1.14
   Compiling tauri-macros v2.6.3
   Compiling hyper v1.11.0
   Compiling codespan-reporting v0.13.1
   Compiling bit-set v0.10.0
   Compiling serialize-to-javascript v0.1.2
   Compiling muda v0.19.3
   Compiling tokio-rustls v0.26.4
   Compiling symphonia-utils-xiph v0.5.5
   Compiling rand_core v0.9.5
   Compiling window-vibrancy v0.6.0
   Compiling half v2.7.1
   Compiling objc2-metal v0.3.2
   Compiling objc2-core-graphics v0.3.2
   Compiling serde_repr v0.1.21
   Compiling ipnet v2.12.1
   Compiling embed_plist v1.2.2
   Compiling rustix v1.1.4
   Compiling static_assertions v1.1.0
   Compiling wgpu-types v30.0.1
   Compiling hyper-util v0.1.20
   Compiling image v0.25.10
   Compiling rand_chacha v0.9.0
   Compiling tower v0.5.3
   Compiling tauri-plugin-fs v2.5.1
   Compiling objc2-quartz-core v0.3.2
   Compiling http-body-util v0.1.5
   Compiling wgpu-hal v30.0.1
   Compiling objc2-core-audio-types v0.3.2
   Compiling raw-window-metal v1.1.0
   Compiling objc2-core-audio v0.3.2
   Compiling objc2 v0.5.2
   Compiling rand v0.9.5
   Compiling security-framework-sys v2.17.0
   Compiling mac-notification-sys v0.6.15
   Compiling libloading v0.8.9
   Compiling profiling v1.0.18
   Compiling cfg_aliases v0.1.1
   Compiling security-framework v3.7.0
   Compiling nix v0.28.0
   Compiling block2 v0.5.1
   Compiling objc2-audio-toolbox v0.3.2
   Compiling tower-http v0.6.11
   Compiling hyper-rustls v0.27.9
   Compiling sha1 v0.10.7
   Compiling tauri-plugin-dialog v2.7.2
   Compiling tauri-plugin-updater v2.10.1
   Compiling wgpu-naga-bridge v30.0.1
   Compiling tauri-plugin-notification v2.3.3
   Compiling tauri-plugin-process v2.3.1
   Compiling tokio-util v0.7.19
   Compiling rand_core v0.6.4
   Compiling wgpu-core v30.0.1
   Compiling num-integer v0.1.47
   Compiling core-foundation v0.9.4
   Compiling curve25519-dalek v4.1.3
   Compiling unicase v2.9.0
   Compiling litrs v1.0.0
   Compiling ryu v1.0.23
   Compiling data-encoding v2.11.1
   Compiling dispatch v0.2.0
   Compiling rfd v0.16.0
   Compiling extended v0.1.0
   Compiling cpal v0.17.3
   Compiling document-features v0.2.12
   Compiling wgpu-core-deps-apple v30.0.1
   Compiling symphonia-format-riff v0.5.5
   Compiling tungstenite v0.29.0
   Compiling objc2-foundation v0.2.2
   Compiling serde_urlencoded v0.7.1
   Compiling core-graphics-types v0.1.3
   Compiling mime_guess v2.0.5
   Compiling num-bigint v0.4.8
   Compiling coreaudio-rs v0.14.2
   Compiling rustls-platform-verifier v0.7.0
   Compiling xattr v1.6.1
   Compiling symphonia-bundle-flac v0.5.5
   Compiling symphonia-codec-vorbis v0.5.5
   Compiling symphonia-format-isomp4 v0.5.5
   Compiling symphonia-format-ogg v0.5.5
   Compiling webpki-roots v0.26.11
   Compiling symphonia-bundle-mp3 v0.5.5
   Compiling symphonia-codec-pcm v0.5.5
   Compiling symphonia-codec-aac v0.5.5
   Compiling objc2-osa-kit v0.3.2
   Compiling winit v0.30.13
   Compiling wgpu v30.0.1
   Compiling mach2 v0.5.0
   Compiling filetime v0.2.29
   Compiling signature v2.2.0
   Compiling dasp_sample v0.11.0
   Compiling tar v0.4.46
   Compiling ed25519 v2.2.3
   Compiling tempfile v3.27.0
   Compiling osakit v0.3.1
   Compiling symphonia v0.5.5
   Compiling tokio-tungstenite v0.29.0
   Compiling reqwest v0.13.4
   Compiling objc2-app-kit v0.2.2
   Compiling num-rational v0.4.2
   Compiling core-graphics v0.23.2
   Compiling notify-rust v4.18.0
   Compiling rand_chacha v0.3.1
   Compiling axum-core v0.5.6
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite/src-tauri)
   Compiling tracing-log v0.2.0
   Compiling tokio-stream v0.1.19
   Compiling sharded-slab v0.1.7
   Compiling filedescriptor v0.8.3
   Compiling notify-types v2.1.0
   Compiling serde_path_to_error v0.1.20
   Compiling fsevent-sys v4.1.0
   Compiling serial2 v0.2.38
   Compiling thread_local v1.1.10
   Compiling shell-words v1.1.1
   Compiling cursor-icon v1.2.0
   Compiling downcast-rs v1.2.1
   Compiling nu-ansi-term v0.50.3
   Compiling matchit v0.8.4
   Compiling smol_str v0.2.2
   Compiling minisign-verify v0.2.5
   Compiling axum v0.8.9
   Compiling tracing-subscriber v0.3.23
   Compiling portable-pty v0.9.0
   Compiling notify v8.2.0
   Compiling tokio-test v0.4.5
   Compiling ed25519-dalek v2.2.0
   Compiling rand v0.8.7
   Compiling rodio v0.22.2
   Compiling reqwest v0.12.28
   Compiling tower-http v0.7.1
   Compiling objc2-user-notifications v0.3.2
   Compiling objc2-core-text v0.3.2
   Compiling base64 v0.23.1
   Compiling pollster v1.0.1
warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:79:28
   |
79 |             let location = unsafe { info.draggingLocation() };
   |                            ^^^^^^ unnecessary `unsafe` block
   |
   = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:80:25
   |
80 |             let point = unsafe { self.convertPoint_fromView(location, None) };
   |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:102:46
    |
102 |     let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };
    |                                              ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:105:25
    |
105 |     if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:122:25
    |
122 |     if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:123:28
    |
123 |         if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
    |                            ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:124:33
    |
124 |             if let Some(path) = unsafe { url.path() } {
    |                                 ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:169:5
    |
169 |     unsafe { view.registerForDraggedTypes(&types) };
    |     ^^^^^^ unnecessary `unsafe` block

warning: unused variable: `super_key`
   --> src/native_terminal/input.rs:366:13
    |
366 |         let super_key = KeyModifiers {
    |             ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_super_key`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
    |
155 |         let mut buffer = vec![0u8; total_pixels];
    |             ----^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `token`
    --> src/remote/auth.rs:1340:14
     |
1340 |         let (token, device) = auth
     |              ^^^^^ help: if this is intentional, prefix it with an underscore: `_token`

warning: field `app` is never read
  --> src/ipc/notifications.rs:28:5
   |
27 | pub struct TauriNotificationBackend<R: Runtime> {
   |            ------------------------ field in this struct
28 |     app: AppHandle<R>,
   |     ^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `no_auth_query` is never used
    --> src/remote/server.rs:2627:12
     |
2627 |         fn no_auth_query() -> AuthQuery {
     |            ^^^^^^^^^^^^^

warning: method `wait_and_reap` is never used
   --> src/terminal/session.rs:270:19
    |
 61 | impl PtySession {
    | --------------- method in this implementation
...
270 |     pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
    |                   ^^^^^^^^^^^^^

warning: struct `WriterLeaseGuard` is never constructed
  --> src/worktree/manager.rs:81:19
   |
81 | pub(crate) struct WriterLeaseGuard {
   |                   ^^^^^^^^^^^^^^^^

warning: associated items `new`, `canonical_path`, and `owner_id` are never used
   --> src/worktree/manager.rs:88:8
    |
 87 | impl WriterLeaseGuard {
    | --------------------- associated items in this implementation
 88 |     fn new(registry: WriterLeaseRegistry, canonical_path: PathBuf, owner_id: String) -> Self {
    |        ^^^
...
 96 |     pub(crate) fn canonical_path(&self) -> &Path {
    |                   ^^^^^^^^^^^^^^
...
100 |     pub(crate) fn owner_id(&self) -> &str {
    |                   ^^^^^^^^

warning: method `acquire_writer_lease` is never used
   --> src/worktree/manager.rs:333:19
    |
129 | impl WorktreeManager {
    | -------------------- method in this implementation
...
333 |     pub(crate) fn acquire_writer_lease(
    |                   ^^^^^^^^^^^^^^^^^^^^

warning: `ferryx` (lib test) generated 17 warnings (run `cargo fix --lib -p ferryx --tests` to apply 3 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 17s
     Running unittests src/lib.rs (/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug/deps/ferryx_lib-1ed65189e94acf09)

running 13 tests
test ipc::ssh::tests::missing_config_file_reports_absence_instead_of_failing ... ok
test ipc::ssh::tests::expand_tilde_path_resolves_home_and_subpaths ... ok
test ipc::ssh::tests::default_store_is_empty ... ok
test ipc::ssh::tests::system_config_result_parses_sample_content ... ok
test ipc::ssh::tests::explicit_config_file_is_read_and_parsed_from_its_own_path ... ok
test ipc::ssh::tests::store_round_trips_through_json ... ok
test ipc::ssh::tests::resilient_store_keeps_latest_duplicate_host_id ... ok
test ipc::ssh::tests::resilient_store_skips_malformed_host_and_retains_valid ... ok

thread 'ipc::ssh::tests::import_preserves_corrupt_store_bytes' (3233277) panicked at src/ipc/ssh.rs:549:9:
assertion `left == right` failed
  left: (None, [123, 10, 32, 32, 34, 104, 111, 115, 116, 115, 34, 58, 32, 91, 10, 32, 32, 32, 32, 123, 10, 32, 32, 32, 32, 32, 32, 34, 105, 100, 34, 58, 32, 34, 115, 115, 104, 45, 110, 101, 119, 45, 98, 111, 120, 34, 44, 10, 32, 32, 32, 32, 32, 32, 34, 108, 97, 98, 101, 108, 34, 58, 32, 34, 110, 101, 119, 45, 98, 111, 120, 34, 44, 10, 32, 32, 32, 32, 32, 32, 34, 104, 111, 115, 116, 110, 97, 109, 101, 34, 58, 32, 34, 110, 101, 119, 46, 101, 120, 97, 109, 112, 108, 101, 34, 44, 10, 32, 32, 32, 32, 32, 32, 34, 115, 111, 117, 114, 99, 101, 34, 58, 32, 34, 99, 111, 110, 102, 105, 103, 34, 44, 10, 32, 32, 32, 32, 32, 32, 34, 97, 117, 116, 104, 77, 101, 116, 104, 111, 100, 34, 58, 32, 34, 97, 103, 101, 110, 116, 34, 10, 32, 32, 32, 32, 125, 10, 32, 32, 93, 44, 10, 32, 32, 34, 116, 111, 109, 98, 115, 116, 111, 110, 101, 115, 34, 58, 32, 91, 93, 10, 125])
 right: (Some(ParseError), [123, 34, 104, 111, 115, 116, 115, 34, 58, 32, 91, 10, 32, 32, 123, 34, 105, 100, 34, 58, 32, 34, 115, 97, 118, 101, 100, 45, 104, 111, 115, 116, 34, 125, 44])
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
test ipc::ssh::tests::import_creates_missing_store ... ok
test ipc::ssh::tests::explicit_import_restores_a_previously_deleted_config_host ... ok
test ipc::ssh::tests::changed_config_alias_replaces_endpoint_without_duplicate_id ... ok
test ipc::ssh::tests::import_preserves_corrupt_store_bytes ... FAILED
test ipc::ssh::tests::repeated_import_preserves_existing_hosts_without_duplicates ... ok

failures:

failures:
    ipc::ssh::tests::import_preserves_corrupt_store_bytes

test result: FAILED. 12 passed; 1 failed; 0 ignored; 0 measured; 1039 filtered out; finished in 0.01s

error: test failed, to rerun pass `--lib`
```

## Full artifact: green-environment.json

SHA256: 2716f121b60e9703358f7d7c6248d20e122a08959692bc75f490d0cf0e969712; UTF-8 bytes: 2352.

```json
{
  "root": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm",
  "repo": "/Users/indo/code/project/orca-lite",
  "shared": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW",
  "env": {
    "PATH": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    "HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/home",
    "USERPROFILE": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/profile",
    "APPDATA": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/appdata",
    "LOCALAPPDATA": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/localappdata",
    "FERRYX_DATA_DIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/data",
    "FERRYX_RUNTIME_DIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/runtime",
    "FERRYX_SESSION_DIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/sessions",
    "TMPDIR": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/temp",
    "TMP": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/temp",
    "TEMP": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/temp",
    "XDG_CACHE_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cache",
    "XDG_CONFIG_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/config",
    "XDG_DATA_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/data",
    "CARGO_HOME": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cargo",
    "CARGO_TARGET_DIR": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target",
    "CARGO_NET_OFFLINE": "true",
    "CARGO_BUILD_JOBS": "8",
    "CARGO_TERM_COLOR": "never",
    "RUSTC_WRAPPER": "",
    "RUSTC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustc",
    "RUSTDOC": "/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/rustdoc",
    "ZIG": "/opt/homebrew/bin/zig",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "LANG": "en_US.UTF-8"
  },
  "policy": "(version 1)(allow default)(deny network*)(deny file-write* (subpath \"/Users/indo/code/project/orca-lite\") (subpath \"/Users/indo/.cargo\") (subpath \"/Users/indo/.rustup\") (subpath \"/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo\"))",
  "command": [
    "cargo",
    "test",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--lib",
    "ipc::ssh::",
    "--",
    "--nocapture"
  ],
  "sourceHash": "6bb4ebd9b62715254ffdec6dcb61489be86ef2417efa51b9c6087bff809de433",
  "startedAt": "2026-09-13T08:17:55.087Z"
}
```

## Full artifact: green-exit.json

SHA256: 6404fa5cded0482b5642a63878f9e240ac416f18ecd31f092d5ae9948146daa6; UTF-8 bytes: 48.

```json
{"code":0,"endedAt":"2026-09-13T08:19:52.555Z"}
```

## Full artifact: green.log

SHA256: 6d17bf9cff8abceabd225805d95eaa63f46f7b0e90c9abb40d09cc042f75fd0e; UTF-8 bytes: 19212.

```text
   Compiling proc-macro2 v1.0.107
   Compiling quote v1.0.47
   Compiling unicode-ident v1.0.24
   Compiling cfg-if v1.0.4
   Compiling libc v0.2.189
   Compiling serde_core v1.0.229
   Compiling shlex v2.0.1
   Compiling find-msvc-tools v0.1.11
   Compiling cc v1.4.3
   Compiling itoa v1.0.18
   Compiling memchr v2.8.3
   Compiling parking_lot_core v0.9.12
   Compiling scopeguard v1.2.0
   Compiling log v0.4.33
   Compiling lock_api v0.4.14
   Compiling writeable v0.6.4
   Compiling litemap v0.8.3
   Compiling bytes v1.12.1
   Compiling utf8_iter v1.0.4
   Compiling siphasher v1.0.3
   Compiling icu_properties_data v2.3.0
   Compiling thiserror v2.0.20
   Compiling icu_normalizer_data v2.3.0
   Compiling phf_shared v0.13.1
   Compiling autocfg v1.5.1
   Compiling syn v3.0.3
   Compiling syn v2.0.119
   Compiling objc2-encode v4.1.0
   Compiling zmij v1.0.23
   Compiling bitflags v2.13.1
   Compiling fastrand v2.5.0
   Compiling phf_generator v0.13.1
   Compiling objc2-exception-helper v0.1.1
   Compiling stable_deref_trait v1.2.1
   Compiling http v1.5.0
   Compiling getrandom v0.4.3
   Compiling objc2 v0.6.4
   Compiling serde v1.0.229
   Compiling smallvec v1.15.2
   Compiling equivalent v1.0.2
   Compiling time-core v0.1.9
   Compiling num-conv v0.2.2
   Compiling winnow v1.0.4
   Compiling powerfmt v0.2.0
   Compiling synstructure v0.13.2
   Compiling once_cell v1.21.4
   Compiling toml_parser v1.1.3+spec-1.1.0
   Compiling toml_writer v1.1.2+spec-1.1.0
   Compiling base64 v0.22.1
   Compiling phf_codegen v0.13.1
   Compiling ident_case v1.0.1
   Compiling strsim v0.11.1
   Compiling serde_derive v1.0.229
   Compiling zerovec-derive v0.11.5
   Compiling displaydoc v0.2.7
   Compiling thiserror-impl v2.0.20
   Compiling serde_json v1.0.151
   Compiling typeid v1.0.3
   Compiling zerofrom-derive v0.1.7
   Compiling yoke-derive v0.8.2
   Compiling phf_macros v0.13.1
   Compiling thiserror v1.0.69
   Compiling block2 v0.6.2
   Compiling zerofrom v0.1.8
   Compiling dispatch2 v0.3.1
   Compiling thiserror-impl v1.0.69
   Compiling darling_core v0.23.0
   Compiling objc2-core-foundation v0.3.2
   Compiling semver v1.0.28
   Compiling erased-serde v0.4.10
   Compiling byteorder v1.5.0
   Compiling aho-corasick v1.1.5
   Compiling objc2-foundation v0.3.2
   Compiling new_debug_unreachable v1.0.6
   Compiling darling_macro v0.23.0
   Compiling unic-common v0.9.0
   Compiling yoke v0.8.3
   Compiling fnv v1.0.7
   Compiling unic-char-range v0.9.0
   Compiling regex-syntax v0.8.11
   Compiling unic-char-property v0.9.0
   Compiling darling v0.23.0
   Compiling unic-ucd-version v0.9.0
   Compiling phf v0.13.1
   Compiling string_cache_codegen v0.6.1
   Compiling alloc-no-stdlib v2.0.4
   Compiling regex-automata v0.4.18
   Compiling anyhow v1.0.104
   Compiling precomputed-hash v0.1.1
   Compiling web_atoms v0.2.6
   Compiling alloc-stdlib v0.2.4
   Compiling parking_lot v0.12.5
   Compiling unic-ucd-ident v0.9.0
   Compiling regex v1.13.1
   Compiling zerovec v0.11.8
   Compiling zerotrie v0.2.5
   Compiling serde_spanned v1.1.1
   Compiling quick-xml v0.41.0
   Compiling serde_with_macros v3.22.0
   Compiling same-file v1.0.6
   Compiling walkdir v2.5.0
   Compiling string_cache v0.9.0
   Compiling brotli-decompressor v5.0.3
   Compiling percent-encoding v2.3.2
   Compiling ctor-proc-macro v0.0.7
   Compiling dunce v1.0.5
   Compiling dtoa v1.0.11
   Compiling ctor v0.8.0
   Compiling dtoa-short v0.3.5
   Compiling brotli v8.0.4
   Compiling form_urlencoded v1.2.2
   Compiling uuid v1.26.0
   Compiling tendril v0.5.1
   Compiling cssparser-macros v0.6.1
   Compiling derive_more-impl v2.1.1
   Compiling selectors v0.36.1
   Compiling tinystr v0.8.4
   Compiling potential_utf v0.1.6
   Compiling indexmap v1.9.3
   Compiling icu_locale_core v2.3.0
   Compiling icu_collections v2.3.0
   Compiling toml_datetime v1.1.1+spec-1.1.0
   Compiling version_check v0.9.5
   Compiling glob v0.3.4
   Compiling camino v1.2.5
   Compiling markup5ever v0.38.0
   Compiling icu_provider v2.3.0
   Compiling toml v1.1.4+spec-1.1.0
   Compiling icu_normalizer v2.3.0
   Compiling icu_properties v2.3.0
   Compiling derive_more v2.1.1
   Compiling cssparser v0.36.0
   Compiling swift-rs v1.0.8
   Compiling idna_adapter v1.2.2
   Compiling idna v1.1.0
   Compiling bytemuck_derive v1.12.0
   Compiling serde_derive_internals v0.29.1
   Compiling servo_arc v0.4.3
   Compiling bit-vec v0.8.0
   Compiling url v2.5.8
   Compiling deranged v0.5.8
   Compiling schemars v0.8.22
   Compiling hashbrown v0.17.1
   Compiling hashbrown v0.12.3
   Compiling rustc-hash v2.1.3
   Compiling bytemuck v1.25.2
   Compiling schemars_derive v0.8.22
   Compiling indexmap v2.14.0
   Compiling time v0.3.55
   Compiling bit-set v0.8.0
   Compiling html5ever v0.38.0
   Compiling cfb v0.7.3
   Compiling jsonptr v0.6.3
   Compiling cargo-platform v0.1.9
   Compiling base64 v0.21.7
   Compiling pin-project-lite v0.2.17
   Compiling dyn-clone v1.0.20
   Compiling bitflags v1.3.2
   Compiling foldhash v0.2.0
   Compiling json-patch v3.0.1
   Compiling infer v0.19.0
   Compiling plist v1.10.0
   Compiling dom_query v0.27.0
   Compiling cargo_metadata v0.19.2
   Compiling serde-untagged v0.1.9
   Compiling urlpattern v0.3.0
   Compiling serde_with v3.22.0
   Compiling errno v0.3.14
   Compiling rustc_version v0.4.1
   Compiling option-ext v0.2.0
   Compiling libm v0.2.16
   Compiling generic-array v0.14.7
   Compiling num-traits v0.2.19
   Compiling signal-hook-registry v1.4.8
   Compiling tokio-macros v2.7.2
   Compiling socket2 v0.6.5
   Compiling tauri-utils v2.9.3
   Compiling mio v1.2.2
   Compiling typenum v1.20.1
   Compiling winnow v0.7.15
   Compiling arrayvec v0.7.8
   Compiling raw-window-handle v0.6.2
   Compiling tokio v1.53.1
   Compiling core-foundation-sys v0.8.7
   Compiling toml_datetime v0.7.5+spec-1.1.0
   Compiling futures-core v0.3.34
   Compiling zerocopy v0.8.56
   Compiling toml v0.9.12+spec-1.1.0
   Compiling block-buffer v0.10.4
   Compiling crypto-common v0.1.7
   Compiling embed-resource v3.0.11
   Compiling dirs-sys v0.5.0
   Compiling zerocopy-derive v0.8.56
   Compiling heck v0.5.0
   Compiling cfg_aliases v0.2.2
   Compiling dirs v6.0.0
   Compiling tauri-winres v0.3.6
   Compiling digest v0.10.7
   Compiling cargo_toml v0.22.3
   Compiling objc2-app-kit v0.3.2
   Compiling getrandom v0.2.17
   Compiling zeroize v1.9.0
   Compiling simd-adler32 v0.3.10
   Compiling crc32fast v1.5.0
   Compiling time-macros v0.2.32
   Compiling adler2 v2.0.1
   Compiling lazy_static v1.5.0
   Compiling miniz_oxide v0.8.9
   Compiling tracing-core v0.1.36
   Compiling ring v0.17.14
   Compiling getrandom v0.3.4
   Compiling flate2 v1.1.9
   Compiling tauri-plugin v2.6.3
   Compiling tauri-build v2.6.3
   Compiling fdeflate v0.3.7
   Compiling rustls-pki-types v1.15.1
   Compiling core-foundation v0.10.1
   Compiling symphonia-core v0.5.5
   Compiling dpi v0.1.2
   Compiling tracing-attributes v0.1.31
   Compiling foreign-types-macros v0.2.4
   Compiling foreign-types-shared v0.3.1
   Compiling subtle v2.6.1
   Compiling untrusted v0.9.0
   Compiling crossbeam-utils v0.8.22
   Compiling foreign-types v0.5.0
   Compiling tracing v0.1.44
   Compiling cookie v0.18.2
   Compiling http-body v1.1.0
   Compiling futures-sink v0.3.34
   Compiling httparse v1.10.1
   Compiling rustls v0.23.43
   Compiling rustls-webpki v0.103.15
   Compiling futures-macro v0.3.34
   Compiling tauri v2.11.5
   Compiling encoding_rs v0.8.35
   Compiling slab v0.4.12
   Compiling tower-service v0.3.3
   Compiling futures-task v0.3.34
   Compiling futures-util v0.3.34
   Compiling symphonia-metadata v0.5.5
   Compiling crossbeam-channel v0.5.16
   Compiling core-graphics-types v0.2.0
   Compiling png v0.17.16
   Compiling objc2-web-kit v0.3.2
   Compiling cpufeatures v0.2.17
   Compiling wry v0.55.1
   Compiling tauri-runtime v2.11.3
   Compiling mime v0.3.17
   Compiling sha2 v0.10.9
   Compiling ico v0.5.0
   Compiling core-graphics v0.25.0
   Compiling ppv-lite86 v0.2.21
   Compiling png v0.18.1
   Compiling try-lock v0.2.5
   Compiling tower-layer v0.3.3
   Compiling rustc-hash v1.1.0
   Compiling pxfm v0.1.30
   Compiling unicode-segmentation v1.13.3
   Compiling tauri-runtime-wry v2.11.4
   Compiling naga-types v30.0.1
   Compiling tao v0.35.3
   Compiling keyboard-types v0.7.0
   Compiling want v0.3.1
   Compiling tauri-codegen v2.6.3
   Compiling webpki-roots v1.0.9
   Compiling naga v30.0.1
   Compiling futures-channel v0.3.34
   Compiling sync_wrapper v1.0.2
   Compiling serialize-to-javascript-impl v0.1.2
   Compiling unicode-width v0.1.14
   Compiling moxcms v0.8.1
   Compiling httpdate v1.0.3
   Compiling byteorder-lite v0.1.0
   Compiling bit-vec v0.9.1
   Compiling objc-sys v0.3.5
   Compiling atomic-waker v1.1.2
   Compiling bit-set v0.10.0
   Compiling hyper v1.11.0
   Compiling tauri-macros v2.6.3
   Compiling serialize-to-javascript v0.1.2
   Compiling codespan-reporting v0.13.1
   Compiling muda v0.19.3
   Compiling tokio-rustls v0.26.4
   Compiling symphonia-utils-xiph v0.5.5
   Compiling half v2.7.1
   Compiling rand_core v0.9.5
   Compiling window-vibrancy v0.6.0
   Compiling objc2-metal v0.3.2
   Compiling objc2-core-graphics v0.3.2
   Compiling serde_repr v0.1.21
   Compiling rustix v1.1.4
   Compiling embed_plist v1.2.2
   Compiling ipnet v2.12.1
   Compiling static_assertions v1.1.0
   Compiling image v0.25.10
   Compiling hyper-util v0.1.20
   Compiling wgpu-types v30.0.1
   Compiling rand_chacha v0.9.0
   Compiling tower v0.5.3
   Compiling objc2-quartz-core v0.3.2
   Compiling tauri-plugin-fs v2.5.1
   Compiling http-body-util v0.1.5
   Compiling wgpu-hal v30.0.1
   Compiling objc2-core-audio-types v0.3.2
   Compiling raw-window-metal v1.1.0
   Compiling objc2-core-audio v0.3.2
   Compiling objc2 v0.5.2
   Compiling rand v0.9.5
   Compiling security-framework-sys v2.17.0
   Compiling mac-notification-sys v0.6.15
   Compiling libloading v0.8.9
   Compiling profiling v1.0.18
   Compiling cfg_aliases v0.1.1
   Compiling security-framework v3.7.0
   Compiling nix v0.28.0
   Compiling block2 v0.5.1
   Compiling objc2-audio-toolbox v0.3.2
   Compiling tower-http v0.6.11
   Compiling wgpu-naga-bridge v30.0.1
   Compiling hyper-rustls v0.27.9
   Compiling sha1 v0.10.7
   Compiling tauri-plugin-process v2.3.1
   Compiling tauri-plugin-notification v2.3.3
   Compiling tauri-plugin-updater v2.10.1
   Compiling tauri-plugin-dialog v2.7.2
   Compiling tokio-util v0.7.19
   Compiling rand_core v0.6.4
   Compiling wgpu-core v30.0.1
   Compiling core-foundation v0.9.4
   Compiling num-integer v0.1.47
   Compiling curve25519-dalek v4.1.3
   Compiling rfd v0.16.0
   Compiling unicase v2.9.0
   Compiling data-encoding v2.11.1
   Compiling ryu v1.0.23
   Compiling cpal v0.17.3
   Compiling dispatch v0.2.0
   Compiling litrs v1.0.0
   Compiling extended v0.1.0
   Compiling symphonia-format-riff v0.5.5
   Compiling document-features v0.2.12
   Compiling tungstenite v0.29.0
   Compiling wgpu-core-deps-apple v30.0.1
   Compiling objc2-foundation v0.2.2
   Compiling serde_urlencoded v0.7.1
   Compiling mime_guess v2.0.5
   Compiling num-bigint v0.4.8
   Compiling core-graphics-types v0.1.3
   Compiling coreaudio-rs v0.14.2
   Compiling rustls-platform-verifier v0.7.0
   Compiling xattr v1.6.1
   Compiling symphonia-format-isomp4 v0.5.5
   Compiling symphonia-format-ogg v0.5.5
   Compiling symphonia-bundle-flac v0.5.5
   Compiling symphonia-codec-vorbis v0.5.5
   Compiling webpki-roots v0.26.11
   Compiling symphonia-bundle-mp3 v0.5.5
   Compiling symphonia-codec-pcm v0.5.5
   Compiling symphonia-codec-aac v0.5.5
   Compiling objc2-osa-kit v0.3.2
   Compiling winit v0.30.13
   Compiling wgpu v30.0.1
   Compiling filetime v0.2.29
   Compiling mach2 v0.5.0
   Compiling dasp_sample v0.11.0
   Compiling signature v2.2.0
   Compiling ed25519 v2.2.3
   Compiling tar v0.4.46
   Compiling tempfile v3.27.0
   Compiling osakit v0.3.1
   Compiling symphonia v0.5.5
   Compiling tokio-tungstenite v0.29.0
   Compiling objc2-app-kit v0.2.2
   Compiling reqwest v0.13.4
   Compiling num-rational v0.4.2
   Compiling core-graphics v0.23.2
   Compiling notify-rust v4.18.0
   Compiling rand_chacha v0.3.1
   Compiling axum-core v0.5.6
   Compiling ferryx v2026.908.1 (/Users/indo/code/project/orca-lite/src-tauri)
   Compiling tracing-log v0.2.0
   Compiling sharded-slab v0.1.7
   Compiling tokio-stream v0.1.19
   Compiling filedescriptor v0.8.3
   Compiling notify-types v2.1.0
   Compiling serde_path_to_error v0.1.20
   Compiling serial2 v0.2.38
   Compiling fsevent-sys v4.1.0
   Compiling thread_local v1.1.10
   Compiling smol_str v0.2.2
   Compiling nu-ansi-term v0.50.3
   Compiling shell-words v1.1.1
   Compiling minisign-verify v0.2.5
   Compiling downcast-rs v1.2.1
   Compiling cursor-icon v1.2.0
   Compiling matchit v0.8.4
   Compiling portable-pty v0.9.0
   Compiling axum v0.8.9
   Compiling tracing-subscriber v0.3.23
   Compiling notify v8.2.0
   Compiling tokio-test v0.4.5
   Compiling ed25519-dalek v2.2.0
   Compiling rand v0.8.7
   Compiling rodio v0.22.2
   Compiling reqwest v0.12.28
   Compiling tower-http v0.7.1
   Compiling objc2-user-notifications v0.3.2
   Compiling objc2-core-text v0.3.2
   Compiling pollster v1.0.1
   Compiling base64 v0.23.1
warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:79:28
   |
79 |             let location = unsafe { info.draggingLocation() };
   |                            ^^^^^^ unnecessary `unsafe` block
   |
   = note: `#[warn(unused_unsafe)]` (part of `#[warn(unused)]`) on by default

warning: unnecessary `unsafe` block
  --> src/macos_file_drop.rs:80:25
   |
80 |             let point = unsafe { self.convertPoint_fromView(location, None) };
   |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:102:46
    |
102 |     let pasteboard: Retained<NSPasteboard> = unsafe { info.draggingPasteboard() };
    |                                              ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:105:25
    |
105 |     if let Some(list) = unsafe { pasteboard.propertyListForType(&legacy) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:122:25
    |
122 |     if let Some(text) = unsafe { pasteboard.stringForType(file_url_type) } {
    |                         ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:123:28
    |
123 |         if let Some(url) = unsafe { NSURL::URLWithString(&text) } {
    |                            ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:124:33
    |
124 |             if let Some(path) = unsafe { url.path() } {
    |                                 ^^^^^^ unnecessary `unsafe` block

warning: unnecessary `unsafe` block
   --> src/macos_file_drop.rs:169:5
    |
169 |     unsafe { view.registerForDraggedTypes(&types) };
    |     ^^^^^^ unnecessary `unsafe` block

warning: unused variable: `super_key`
   --> src/native_terminal/input.rs:366:13
    |
366 |         let super_key = KeyModifiers {
    |             ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_super_key`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
    |
155 |         let mut buffer = vec![0u8; total_pixels];
    |             ----^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `token`
    --> src/remote/auth.rs:1340:14
     |
1340 |         let (token, device) = auth
     |              ^^^^^ help: if this is intentional, prefix it with an underscore: `_token`

warning: field `app` is never read
  --> src/ipc/notifications.rs:28:5
   |
27 | pub struct TauriNotificationBackend<R: Runtime> {
   |            ------------------------ field in this struct
28 |     app: AppHandle<R>,
   |     ^^^
   |
   = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: function `no_auth_query` is never used
    --> src/remote/server.rs:2627:12
     |
2627 |         fn no_auth_query() -> AuthQuery {
     |            ^^^^^^^^^^^^^

warning: method `wait_and_reap` is never used
   --> src/terminal/session.rs:270:19
    |
 61 | impl PtySession {
    | --------------- method in this implementation
...
270 |     pub(crate) fn wait_and_reap(&self) -> Result<Option<i32>, PtyError> {
    |                   ^^^^^^^^^^^^^

warning: struct `WriterLeaseGuard` is never constructed
  --> src/worktree/manager.rs:81:19
   |
81 | pub(crate) struct WriterLeaseGuard {
   |                   ^^^^^^^^^^^^^^^^

warning: associated items `new`, `canonical_path`, and `owner_id` are never used
   --> src/worktree/manager.rs:88:8
    |
 87 | impl WriterLeaseGuard {
    | --------------------- associated items in this implementation
 88 |     fn new(registry: WriterLeaseRegistry, canonical_path: PathBuf, owner_id: String) -> Self {
    |        ^^^
...
 96 |     pub(crate) fn canonical_path(&self) -> &Path {
    |                   ^^^^^^^^^^^^^^
...
100 |     pub(crate) fn owner_id(&self) -> &str {
    |                   ^^^^^^^^

warning: method `acquire_writer_lease` is never used
   --> src/worktree/manager.rs:333:19
    |
129 | impl WorktreeManager {
    | -------------------- method in this implementation
...
333 |     pub(crate) fn acquire_writer_lease(
    |                   ^^^^^^^^^^^^^^^^^^^^

warning: `ferryx` (lib test) generated 17 warnings (run `cargo fix --lib -p ferryx --tests` to apply 3 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 52.87s
     Running unittests src/lib.rs (/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target/debug/deps/ferryx_lib-1ed65189e94acf09)

running 13 tests
test ipc::ssh::tests::missing_config_file_reports_absence_instead_of_failing ... ok
test ipc::ssh::tests::default_store_is_empty ... ok
test ipc::ssh::tests::expand_tilde_path_resolves_home_and_subpaths ... ok
test ipc::ssh::tests::system_config_result_parses_sample_content ... ok
test ipc::ssh::tests::explicit_config_file_is_read_and_parsed_from_its_own_path ... ok
test ipc::ssh::tests::resilient_store_keeps_latest_duplicate_host_id ... ok
test ipc::ssh::tests::store_round_trips_through_json ... ok
test ipc::ssh::tests::import_preserves_corrupt_store_bytes ... ok
test ipc::ssh::tests::resilient_store_skips_malformed_host_and_retains_valid ... ok
test ipc::ssh::tests::import_creates_missing_store ... ok
test ipc::ssh::tests::explicit_import_restores_a_previously_deleted_config_host ... ok
test ipc::ssh::tests::repeated_import_preserves_existing_hosts_without_duplicates ... ok
test ipc::ssh::tests::changed_config_alias_replaces_endpoint_without_duplicate_id ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 1039 filtered out; finished in 0.01s

```

## Raw cleanup receipt

```json
{
  "at": "2026-09-13T08:25:31.572Z",
  "action": "rmSync exact P11 owned root, recursive; symlinks unlinked, not followed",
  "root": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm",
  "archivedArtifactsVerified": [
    "run.mjs",
    "red-environment.json",
    "red-exit.json",
    "red.log",
    "green-environment.json",
    "green-exit.json",
    "green.log"
  ],
  "tempRemainingBeforeCleanup": [],
  "inventory": [
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/home",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/temp",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/run.mjs",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cache",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/red.log",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/config",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cargo",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cargo/.package-cache",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cargo/.package-cache-mutate",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cargo/.global-cache",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/cargo/registry",
      "type": "symlink",
      "target": "/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/cargo/registry"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/green.log",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/red-exit.json",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/runtime",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/localappdata",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/green-exit.json",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/sessions",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/profile",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/appdata",
      "type": "directory"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/red-environment.json",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/green-environment.json",
      "type": "file"
    },
    {
      "path": "/private/tmp/ferryx-p11-st_01a099d0.OHtDkm/data",
      "type": "directory"
    }
  ],
  "ownedRootAbsent": true,
  "sharedTargetExists": true,
  "sharedTargetDeleted": false
}
```
