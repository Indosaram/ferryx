use super::state::RemoteGatewayState;
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::ssh::projects::{self, RemoteProject};
use crate::ssh::SshHost;
use std::collections::BTreeMap;

pub(super) async fn projects(state: &RemoteGatewayState) -> Result<Vec<RemoteProject>, IpcError> {
    Ok(labeled_projects(state)
        .await?
        .into_iter()
        .map(|(project, _)| project)
        .collect())
}

/// Enabled SSH projects paired with their host's sanitized display label.
pub(super) async fn labeled_projects(
    state: &RemoteGatewayState,
) -> Result<Vec<(RemoteProject, Option<String>)>, IpcError> {
    let Some(store) = state.ssh_store_path.read().clone() else {
        return Ok(Vec::new());
    };
    run_blocking(move || {
        let bytes = match std::fs::read(projects::store_path(&store)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(IpcError::new(IpcErrorCode::IoError, error.to_string())),
        };
        let saved: BTreeMap<String, RemoteProject> = serde_json::from_slice(&bytes)
            .map_err(|error| IpcError::new(IpcErrorCode::ParseError, error.to_string()))?;
        let mut enabled = Vec::new();
        for id in saved.keys() {
            match projects::resolve(&store, id) {
                Ok((project, host)) => enabled.push((project, host_label(&host))),
                Err(error) if error.code == IpcErrorCode::WorkspaceNotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(enabled)
    })
    .await
}

fn host_label(host: &SshHost) -> Option<String> {
    let label = host.label.trim();
    (!label.is_empty() && !label.chars().any(char::is_control)).then(|| label.to_owned())
}

pub(super) fn label(project: &RemoteProject) -> String {
    project
        .repo_root
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty() && !name.chars().any(char::is_control))
        .unwrap_or("SSH")
        .to_owned()
}
