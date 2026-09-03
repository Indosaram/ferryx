use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandoverRoute {
    pub legacy_socket_path: PathBuf,
    pub sessions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandoverManifest {
    pub routes: Vec<HandoverRoute>,
}

impl HandoverManifest {
    pub fn load_from_path(path: &Path) -> Self {
        let Ok(data) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        let tmp_path = path.with_extension(format!("tmp-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
        fs::write(&tmp_path, json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
        }
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn prune_dead_routes(&mut self) {
        self.routes
            .retain(|route| route.legacy_socket_path.exists());
    }

    pub fn add_or_update_route(&mut self, route: HandoverRoute) {
        self.routes
            .retain(|r| r.legacy_socket_path != route.legacy_socket_path);
        self.routes.push(route);
    }

    pub fn remove_route(&mut self, legacy_socket_path: &Path) {
        self.routes
            .retain(|r| r.legacy_socket_path != legacy_socket_path);
    }
}

pub fn get_manifest_path() -> PathBuf {
    crate::daemon::server::get_runtime_dir().join("handover_routes.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_handover_manifest_save_load_prune() {
        let dir = tempdir().unwrap();
        let manifest_path = dir.path().join("routes.json");

        let existing_sock = dir.path().join("legacy1.sock");
        fs::write(&existing_sock, "").unwrap();

        let dead_sock = dir.path().join("dead.sock");

        let mut manifest = HandoverManifest::default();
        manifest.add_or_update_route(HandoverRoute {
            legacy_socket_path: existing_sock.clone(),
            sessions: vec!["s1".to_string()],
        });
        manifest.add_or_update_route(HandoverRoute {
            legacy_socket_path: dead_sock.clone(),
            sessions: vec!["s2".to_string()],
        });

        manifest.save_to_path(&manifest_path).unwrap();

        let mut loaded = HandoverManifest::load_from_path(&manifest_path);
        assert_eq!(loaded.routes.len(), 2);

        loaded.prune_dead_routes();
        assert_eq!(loaded.routes.len(), 1);
        assert_eq!(loaded.routes[0].legacy_socket_path, existing_sock);
    }
}
