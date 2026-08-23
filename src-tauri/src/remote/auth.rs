use parking_lot::RwLock;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PAIRING_EXPIRY: Duration = Duration::from_secs(60);
const LAST_SEEN_PERSIST_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DevicePermission {
    View,
    Control,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub permission: DevicePermission,
    pub created_at: u64,
    pub last_seen_at: u64,
    pub revoked: bool,
}

struct PairingCode {
    _code: String,
    created_at: Instant,
    default_permission: DevicePermission,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedAuthState {
    devices: HashMap<String, DeviceInfo>,
    tokens: HashMap<String, String>,
}

#[derive(Clone)]
pub struct AuthManager {
    pairing_codes: Arc<RwLock<HashMap<String, PairingCode>>>,
    devices: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    tokens: Arc<RwLock<HashMap<String, String>>>,
    persistence_path: Option<PathBuf>,
    last_persisted_at: Arc<RwLock<Instant>>,
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthManager {
    pub fn new() -> Self {
        Self::with_persistence(None)
    }

    pub fn with_persistence(persistence_path: Option<PathBuf>) -> Self {
        let persisted = persistence_path
            .as_deref()
            .and_then(load_persisted_auth)
            .unwrap_or_default();
        Self {
            pairing_codes: Arc::new(RwLock::new(HashMap::new())),
            devices: Arc::new(RwLock::new(persisted.devices)),
            tokens: Arc::new(RwLock::new(persisted.tokens)),
            persistence_path,
            last_persisted_at: Arc::new(RwLock::new(Instant::now())),
        }
    }

    pub fn create_pairing_code(&self, default_permission: DevicePermission) -> String {
        let pin: u32 = rand::thread_rng().gen_range(100_000..=999_999);
        let code = format!("{pin:06}");

        self.pairing_codes.write().insert(
            code.clone(),
            PairingCode {
                _code: code.clone(),
                created_at: Instant::now(),
                default_permission,
            },
        );
        code
    }

    pub fn exchange_pairing_code(
        &self,
        code: &str,
        device_name: &str,
    ) -> Result<(String, DeviceInfo), AuthError> {
        let mut codes = self.pairing_codes.write();
        let pairing = codes.remove(code).ok_or(AuthError::InvalidPairingCode)?;
        drop(codes);

        if pairing.created_at.elapsed() > PAIRING_EXPIRY {
            return Err(AuthError::ExpiredPairingCode);
        }

        let device_id = uuid::Uuid::new_v4().to_string();
        let token: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        let now = unix_now();
        let info = DeviceInfo {
            id: device_id.clone(),
            name: device_name.to_string(),
            permission: pairing.default_permission,
            created_at: now,
            last_seen_at: now,
            revoked: false,
        };

        self.devices.write().insert(device_id.clone(), info.clone());
        self.tokens.write().insert(token.clone(), device_id);
        self.persist_best_effort();
        Ok((token, info))
    }

    pub fn validate_token(&self, token: &str) -> Result<DeviceInfo, AuthError> {
        let device_id = {
            let tokens = self.tokens.read();
            tokens.get(token).cloned()
        }
        .ok_or(AuthError::Unauthorized)?;

        let result = {
            let mut devices = self.devices.write();
            let device = devices.get_mut(&device_id).ok_or(AuthError::Unauthorized)?;
            if device.revoked {
                return Err(AuthError::RevokedDevice);
            }
            device.last_seen_at = unix_now();
            device.clone()
        };

        let should_persist = self.last_persisted_at.read().elapsed() >= LAST_SEEN_PERSIST_INTERVAL;
        if should_persist {
            self.persist_best_effort();
        }

        Ok(result)
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.devices.read().values().cloned().collect()
    }

    pub fn revoke_device(&self, device_id: &str) -> bool {
        let changed = {
            let mut devices = self.devices.write();
            if let Some(device) = devices.get_mut(device_id) {
                device.revoked = true;
                true
            } else {
                false
            }
        };
        if changed {
            self.persist_best_effort();
        }
        changed
    }

    #[cfg(test)]
    pub(crate) fn set_last_persisted_at(&self, instant: Instant) {
        *self.last_persisted_at.write() = instant;
    }

    fn persist_best_effort(&self) {
        let Some(path) = self.persistence_path.as_deref() else {
            return;
        };
        *self.last_persisted_at.write() = Instant::now();
        let snapshot = PersistedAuthState {
            devices: self.devices.read().clone(),
            tokens: self.tokens.read().clone(),
        };
        if let Err(error) = write_private_json(path, &snapshot) {
            tracing::warn!("failed to persist remote auth state: {error}");
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load_persisted_auth(path: &Path) -> Option<PersistedAuthState> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn write_private_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let temp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(&temp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(temp, path)?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid pairing code")]
    InvalidPairingCode,
    #[error("Pairing code has expired")]
    ExpiredPairingCode,
    #[error("Unauthorized access")]
    Unauthorized,
    #[error("Device access has been revoked")]
    RevokedDevice,
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn paired_devices_and_revocations_survive_reopen() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("remote-auth.json");
        let manager = AuthManager::with_persistence(Some(path.clone()));
        let code = manager.create_pairing_code(DevicePermission::Control);
        let (token, device) = manager.exchange_pairing_code(&code, "Phone").expect("pair");
        assert!(manager.revoke_device(&device.id));

        let reopened = AuthManager::with_persistence(Some(path));
        let listed = reopened.list_devices();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].revoked);
        assert!(matches!(
            reopened.validate_token(&token),
            Err(AuthError::RevokedDevice)
        ));
    }

    #[test]
    fn validate_token_throttles_disk_persistence() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("remote-auth.json");
        let manager = AuthManager::with_persistence(Some(path.clone()));
        let code = manager.create_pairing_code(DevicePermission::Control);
        let (token, device) = manager.exchange_pairing_code(&code, "Phone").expect("pair");
        assert!(path.exists(), "pairing must persist immediately");

        // Remove the file on disk to observe whether validate_token writes to disk
        std::fs::remove_file(&path).expect("remove file");
        assert!(!path.exists());

        // Validating token right after pairing must not rewrite to disk (<60s)
        let validated = manager.validate_token(&token).expect("validate");
        assert_eq!(validated.name, "Phone");
        assert!(
            !path.exists(),
            "validate_token must not persist to disk on every request"
        );

        // When >= 60s have elapsed since last persist, validate_token flushes to disk
        manager.set_last_persisted_at(
            Instant::now()
                .checked_sub(Duration::from_secs(65))
                .expect("instant subtraction"),
        );
        let validated_after = manager
            .validate_token(&token)
            .expect("validate after interval");
        assert_eq!(validated_after.name, "Phone");
        assert!(
            path.exists(),
            "validate_token must persist to disk once >=60s has elapsed"
        );

        // Revoking must persist immediately
        std::fs::remove_file(&path).expect("remove file before revoke");
        assert!(!path.exists());
        assert!(manager.revoke_device(&device.id));
        assert!(path.exists(), "revoke_device must persist immediately");
    }
}
