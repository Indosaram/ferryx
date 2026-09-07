//! Durable remote identities, separate from the local workspace/Git registry.
use super::{direct, SshHost};
use crate::ipc::{ssh::SshHostStore, IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static MUTATION: Mutex<()> = Mutex::new(());
pub const REMOTE_PREFIX: &str = "ssh:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteProject {
    pub workspace_id: String,
    pub host_id: String,
    pub repo_root: String,
    pub git_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
}

pub fn is_remote(id: &str) -> bool {
    id.starts_with(REMOTE_PREFIX)
}

pub fn unsupported() -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        "This operation is unavailable for direct SSH projects",
    )
}

fn io(error: std::io::Error) -> IpcError {
    IpcError::new(IpcErrorCode::IoError, error.to_string())
}

pub fn store_path(host_store: &Path) -> PathBuf {
    host_store.with_file_name("remote_projects.json")
}

fn read_projects(host_store: &Path) -> Result<BTreeMap<String, RemoteProject>, IpcError> {
    match std::fs::read(store_path(host_store)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| IpcError::new(IpcErrorCode::ParseError, e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(io(e)),
    }
}

pub fn enabled_host(host_store: &Path, host_id: &str) -> Result<SshHost, IpcError> {
    let store: SshHostStore = match std::fs::read(host_store) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| IpcError::new(IpcErrorCode::ParseError, e.to_string()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SshHostStore::default(),
        Err(e) => return Err(io(e)),
    };
    let host = store
        .hosts
        .into_iter()
        .find(|h| h.id == host_id)
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::WorkspaceNotFound,
                "Configured SSH host no longer exists",
            )
            .with_details(serde_json::json!({"hostId": host_id, "reason": "hostMissing"}))
        })?;
    if host.disabled == Some(true) {
        return Err(IpcError::new(
            IpcErrorCode::WorkspaceNotFound,
            "Configured SSH host is disabled",
        )
        .with_details(serde_json::json!({"hostId": host_id, "reason": "hostDisabled"})));
    }
    direct::validate_host(&host)?;
    Ok(host)
}

pub fn identity(host_id: &str, root: &str) -> String {
    let mut hash = Sha256::new();
    hash.update((host_id.len() as u64).to_le_bytes());
    hash.update(host_id.as_bytes());
    hash.update(root.as_bytes());
    format!("{REMOTE_PREFIX}{:x}", hash.finalize())
}

pub fn resolve(
    host_store: &Path,
    workspace_id: &str,
) -> Result<(RemoteProject, SshHost), IpcError> {
    let projects = read_projects(host_store)?;
    let project = projects.get(workspace_id).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::WorkspaceNotFound,
            "Remote project is not registered",
        )
    })?;
    if project.workspace_id != workspace_id
        || identity(&project.host_id, &project.repo_root) != workspace_id
    {
        return Err(IpcError::new(
            IpcErrorCode::ParseError,
            "Remote project identity does not match its stored location",
        ));
    }
    direct::validate_remote_path(&project.repo_root)?;
    let host = enabled_host(host_store, &project.host_id)?;
    Ok((project.clone(), host))
}

fn save(host_store: &Path, projects: &BTreeMap<String, RemoteProject>) -> Result<(), IpcError> {
    let path = store_path(host_store);
    let bytes =
        serde_json::to_vec_pretty(projects).map_err(|e| IpcError::internal(e.to_string()))?;
    // The host store's parent already exists. A unique temp file never clobbers another writer.
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)
    })();
    if let Err(error) = result {
        match std::fs::remove_file(&temp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(io(std::io::Error::other(format!(
                    "{error}; temp cleanup failed: {e}"
                ))))
            }
        }
        return Err(io(error));
    }
    Ok(())
}

pub fn persist(host_store: &Path, project: RemoteProject) -> Result<RemoteProject, IpcError> {
    let _guard = MUTATION
        .lock()
        .map_err(|e| IpcError::internal(e.to_string()))?;
    // Re-resolve after the network probe; deletion/disable during registration fails closed.
    enabled_host(host_store, &project.host_id)?;
    let mut projects = read_projects(host_store)?;
    if let Some(existing) = projects.get(&project.workspace_id) {
        if existing.host_id != project.host_id || existing.repo_root != project.repo_root {
            return Err(IpcError::new(
                IpcErrorCode::WorkspaceAlreadyRegistered,
                "Remote identity collision",
            ));
        }
    }
    projects.insert(project.workspace_id.clone(), project.clone());
    save(host_store, &projects)?;
    Ok(project)
}

pub fn unregister(host_store: &Path, workspace_id: &str) -> Result<(), IpcError> {
    let _guard = MUTATION
        .lock()
        .map_err(|e| IpcError::internal(e.to_string()))?;
    let mut projects = read_projects(host_store)?;
    if projects.remove(workspace_id).is_some() {
        save(host_store, &projects)?;
    }
    Ok(())
}

/// Provenance comes only from explicit workspace context. A legacy path-only
/// request remains local, regardless of paths saved for remote projects.
pub fn guard_reveal(workspace_id: Option<&str>) -> Result<(), IpcError> {
    if workspace_id.is_some_and(is_remote) {
        return Err(unsupported());
    }
    Ok(())
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
