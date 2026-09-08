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
    pub fn update_at_path(
        path: &Path,
        update: impl FnOnce(&mut Self),
    ) -> Result<Self, std::io::Error> {
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options.open(path.with_extension("lock"))?;
        lock.lock()?;
        let mut manifest = match fs::read(path) {
            Ok(data) => serde_json::from_slice(&data)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error),
        };
        update(&mut manifest);
        manifest.save_to_path(path)?;
        Ok(manifest)
    }

    pub fn load_from_path(path: &Path) -> Self {
        let Ok(data) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        let tmp_path = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
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
    fn handover_manifest_update_excludes_other_file_handles() {
        let dir = tempdir().expect("isolated manifest directory");
        let path = dir.path().join("routes.json");
        let competing_writer = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .expect("competing writer lock");

        HandoverManifest::update_at_path(&path, |manifest| {
            assert!(
                matches!(
                    competing_writer.try_lock(),
                    Err(fs::TryLockError::WouldBlock)
                ),
                "another writer can overwrite the transaction's manifest snapshot"
            );
            manifest.add_or_update_route(HandoverRoute {
                legacy_socket_path: dir.path().join("legacy.sock"),
                sessions: vec!["existing-session".into()],
            });
        })
        .expect("serialized update");

        assert_eq!(HandoverManifest::load_from_path(&path).routes.len(), 1);
        competing_writer.try_lock().expect("transaction released lock");
    }

    #[test]
    fn handover_manifest_update_preserves_unreadable_route_data() {
        let dir = tempdir().expect("isolated manifest directory");
        let path = dir.path().join("routes.json");
        fs::write(&path, b"{broken manifest").expect("invalid manifest fixture");

        let result = HandoverManifest::update_at_path(&path, |manifest| {
            manifest.prune_dead_routes();
        });

        assert!(result.is_err(), "corrupt routes must not become an empty manifest");
        assert_eq!(fs::read(&path).expect("preserved bytes"), b"{broken manifest");
    }

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
