//! Lossless native project projection. No catalog or second inventory authority.
use crate::{remote::machine_protocol as m, scoped_contracts::RunTarget};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    #[serde(flatten)]
    pub metadata: m::Project,
    pub remote_workspace_id: String,
    pub target: RunTarget,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Projects {
    pub revision: crate::scoped_contracts::Epoch,
    pub completeness: m::Completeness,
    pub projects: Vec<Project>,
    pub unavailable_workspace_ids: Vec<String>,
}
pub fn project(host: &str, mut metadata: m::Project) -> Project {
    let remote_workspace_id = metadata.workspace_id.clone();
    metadata.workspace_id = id(host, &remote_workspace_id);
    Project {
        metadata,
        remote_workspace_id,
        target: RunTarget::PairedDaemon {
            host_id: host.into(),
        },
    }
}
pub fn id(host: &str, remote: &str) -> String {
    m::desktop_workspace_id(host, remote).expect("string identity tuple")
}
pub fn projects(host: &str, rows: m::Projects) -> Projects {
    Projects {
        revision: rows.revision,
        completeness: rows.completeness,
        projects: rows
            .projects
            .into_iter()
            .map(|p| project(host, p))
            .collect(),
        unavailable_workspace_ids: rows
            .unavailable_workspace_ids
            .iter()
            .map(|p| id(host, p))
            .collect(),
    }
}

/// Recover only a uniquely registered path from a complete inventory of the same host.
pub fn recover_remote_id<'a>(
    rows: &'a Projects,
    host: &str,
    remote_id: &str,
    repo_root: &str,
) -> Option<&'a str> {
    if rows.completeness != m::Completeness::Complete || repo_root.is_empty() {
        return None;
    }
    let same_host = |p: &&Project| matches!(&p.target,
        RunTarget::PairedDaemon { host_id } if host_id == host);
    if rows.projects.iter().filter(same_host).any(|p| p.remote_workspace_id == remote_id) {
        return None;
    }
    let mut matches = rows.projects.iter().filter(same_host).filter(|p| {
        p.metadata.repo_root == repo_root
            && p.metadata.availability == m::Availability::Ready
            && !rows.unavailable_workspace_ids.contains(&p.metadata.workspace_id)
    });
    let found = matches.next()?;
    matches.next().is_none().then_some(found.remote_workspace_id.as_str())
}

pub fn store_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("paired_projects.json")
}

pub fn read_stored_projects(
    data_dir: &std::path::Path,
) -> std::collections::BTreeMap<String, Project> {
    let path = store_path(data_dir);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => std::collections::BTreeMap::new(),
    }
}

pub fn save_stored_project(data_dir: &std::path::Path, p: Project) -> Result<(), std::io::Error> {
    let mut map = read_stored_projects(data_dir);
    map.insert(p.metadata.workspace_id.clone(), p);
    let path = store_path(data_dir);
    let bytes = serde_json::to_vec_pretty(&map)?;
    std::fs::write(path, bytes)
}

pub fn resolve_stored_project(data_dir: &std::path::Path, workspace_id: &str) -> Option<Project> {
    let map = read_stored_projects(data_dir);
    map.get(workspace_id).cloned()
}
#[cfg(all(test, feature = "desktop"))]
#[path = "projects_tests.rs"]
mod tests;
