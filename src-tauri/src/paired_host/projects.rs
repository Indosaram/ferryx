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
#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
