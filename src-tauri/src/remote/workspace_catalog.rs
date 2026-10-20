//! Private durable registration state, independent of GUI and SSH snapshots.
use crate::{remote::machine_protocol::Availability, scoped_contracts::Epoch};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogRow {
    pub repo_root: PathBuf,
    pub mirror_exposed: bool,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopLocalProject {
    pub workspace_id: String,
    pub repo_root: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ownership {
    pub desktop: bool,
    pub machine: bool,
    pub legacy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub revision: Epoch,
    pub workspaces: BTreeMap<String, CatalogRow>,
    pub desktop_local_projects: Option<Vec<DesktopLocalProject>>,
    pub ownership: BTreeMap<String, Ownership>,
    /// Last observed Git state, committed with the revision that fences deletion.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub worktree_observations: BTreeMap<String, String>,
    /// Atomic proof of the most recent catalog transaction, not a second journal.
    /// The next commit is admitted only after this receipt reaches the journal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<super::machine_operation_journal::Record>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 2,
            revision: Epoch(0),
            workspaces: BTreeMap::new(),
            desktop_local_projects: None,
            ownership: BTreeMap::new(),
            worktree_observations: BTreeMap::new(),
            transaction: None,
        }
    }
}

/// Decode each supported envelope explicitly. V1 rows have unknown provenance,
/// not inferred desktop ownership, and retain every journal/observation field.
fn decode(bytes: &[u8]) -> Result<Catalog, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct V1 {
        version: u32,
        revision: Epoch,
        workspaces: BTreeMap<String, CatalogRow>,
        #[serde(default)]
        worktree_observations: BTreeMap<String, String>,
        #[serde(default)]
        transaction: Option<super::machine_operation_journal::Record>,
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(1) => {
            let old: V1 = serde_json::from_value(value).map_err(|e| e.to_string())?;
            debug_assert_eq!(old.version, 1);
            let ownership = old.workspaces.keys().map(|id| (id.clone(), Ownership {
                legacy: true, ..Ownership::default()
            })).collect();
            Ok(Catalog {
                version: 2, revision: old.revision, workspaces: old.workspaces,
                worktree_observations: old.worktree_observations,
                transaction: old.transaction, desktop_local_projects: None, ownership,
            })
        }
        Some(2) => serde_json::from_value(value).map_err(|e| e.to_string()),
        _ => Err("Unsupported catalog version".into()),
    }
}

pub(crate) fn load(path: &Path) -> Result<Catalog, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Catalog::default()),
        Err(e) => return Err(e.to_string()),
    };
    let parsed = decode(&bytes)
        .and_then(|catalog| {
            for (id, row) in &catalog.workspaces {
                crate::worktree::WorkspaceRegistry::validate_workspace_id(id)
                    .map_err(|e| e.to_string())?;
                if !row.repo_root.is_absolute() || row.repo_root.parent().is_none() {
                    return Err("Invalid catalog root".into());
                }
            }
            if let Some(projects) = &catalog.desktop_local_projects {
                let mut ids = std::collections::BTreeSet::new();
                for project in projects {
                    crate::worktree::WorkspaceRegistry::validate_workspace_id(&project.workspace_id)
                        .map_err(|e| e.to_string())?;
                    if !ids.insert(&project.workspace_id) || !project.repo_root.is_absolute()
                        || project.repo_root.parent().is_none() {
                        return Err("Invalid desktop membership".into());
                    }
                }
            }
            Ok(catalog)
        });
    if parsed.is_err() {
        // Preserve the original as a persistent fail-closed marker, and keep a
        // diagnostic copy. A restart must not turn quarantine into an empty store.
        let quarantine = path.with_extension(format!("quarantine.{}", uuid::Uuid::new_v4()));
        super::auth::write_private_json(&quarantine, &serde_json::json!({"originalBytes": bytes}))
            .map_err(|e| format!("Catalog quarantine failed: {e}"))?;
    }
    parsed
}

pub(crate) fn persist(path: &Path, catalog: &Catalog) -> Result<(), String> {
    super::auth::write_private_json(path, catalog).map_err(|e| e.to_string())?;
    #[cfg(test)]
    if FAIL_DIRECTORY_SYNC.with(|fail| fail.replace(false)) {
        return Err("Injected post-rename directory sync failure".into());
    }
    // The shared writer tolerates directory-sync failures for legacy stores.
    // Catalog admission must not: an ambiguous commit fences further mutations.
    #[cfg(unix)]
    std::fs::File::open(path.parent().ok_or("Catalog has no parent")?)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAIL_DIRECTORY_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn v1_migration_retains_legacy_rows_observations_and_empty_authority_distinction() {
        crate::ipc::run_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("catalog");
            let missing = root.path().join("missing-volume/repo");
            let user_temp = root.path().join("user-temp");
            let home = root.path().join("home");
            std::fs::create_dir(&user_temp).unwrap();
            std::fs::create_dir(&home).unwrap();
            let user_temp = std::fs::canonicalize(user_temp).unwrap();
            let old = serde_json::json!({"version":1,"revision":"7","workspaces":{
                "external":{"repoRoot":missing,"mirrorExposed":true,"availability":"missing"},
                "user-temp":{"repoRoot":user_temp,"mirrorExposed":true,"availability":"ready"},
                "leaked-temp":{"repoRoot":root.path().join("gone"),"mirrorExposed":false,"availability":"missing"},
                "machine":{"repoRoot":root.path().join("machine"),"mirrorExposed":false,"availability":"missing"},
                "home":{"repoRoot":home,"mirrorExposed":true,"availability":"ready"}
            },"worktreeObservations":{"external":"digest"}});
            super::super::auth::write_private_json(&path, &old).unwrap();
            let migrated = super::load(&path).unwrap();
            assert_eq!(migrated.version, 2);
            assert_eq!(migrated.revision.0, 7);
            assert_eq!(migrated.workspaces["external"].repo_root, missing);
            assert_eq!(migrated.worktree_observations["external"], "digest");
            assert!(migrated.ownership["external"].legacy);
            assert!(migrated.desktop_local_projects.is_none());
            assert_eq!(migrated.workspaces.len(), 5);
            super::persist(&path, &migrated).unwrap();
            assert_eq!(super::load(&path).unwrap().version, 2);
            let service = crate::daemon::workspace_service::DaemonWorkspaceService::new(crate::worktree::WorkspaceRegistry::new(), path);
            service.sync_desktop_local_projects(vec![
                super::DesktopLocalProject { workspace_id: "external".into(), repo_root: missing },
                super::DesktopLocalProject { workspace_id: "user-temp".into(), repo_root: user_temp.clone() },
            ]).unwrap();
            let synced = service.catalog().unwrap();
            assert_eq!(synced.workspaces.len(), 5);
            assert!(synced.ownership.values().all(|owner| owner.legacy));
            assert_eq!(synced.desktop_local_projects.unwrap().len(), 2);
            assert!(user_temp.is_dir() && home.is_dir());
            drop(service);
            root.close().unwrap();
            Ok(())
        }).await.unwrap();
    }
}
