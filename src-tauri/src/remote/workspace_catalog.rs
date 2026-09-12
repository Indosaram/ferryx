//! Private durable registration state, independent of GUI and SSH snapshots.
use crate::{remote::machine_protocol::Availability, scoped_contracts::Epoch};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogRow {
    pub repo_root: PathBuf,
    pub mirror_exposed: bool,
    pub availability: Availability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub revision: Epoch,
    pub workspaces: BTreeMap<String, CatalogRow>,
}

impl Default for Catalog {
    fn default() -> Self { Self { version: 1, revision: Epoch(0), workspaces: BTreeMap::new() } }
}

pub(crate) fn load(path: &Path) -> Result<Catalog, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Catalog::default()),
        Err(e) => return Err(e.to_string()),
    };
    let parsed = serde_json::from_slice::<Catalog>(&bytes).map_err(|e| e.to_string())
        .and_then(|catalog| {
            if catalog.version != 1 { return Err("Unsupported catalog version".into()); }
            for (id, row) in &catalog.workspaces {
                crate::worktree::WorkspaceRegistry::validate_workspace_id(id).map_err(|e| e.to_string())?;
                if !row.repo_root.is_absolute() || row.repo_root.parent().is_none() {
                    return Err("Invalid catalog root".into());
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
        .and_then(|directory| directory.sync_all()).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAIL_DIRECTORY_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
