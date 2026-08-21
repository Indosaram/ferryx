use parking_lot::RwLock;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PAIRING_EXPIRY: Duration = Duration::from_secs(300); // 5 minutes

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

#[derive(Clone)]
pub struct AuthManager {
    pairing_codes: Arc<RwLock<HashMap<String, PairingCode>>>,
    devices: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    tokens: Arc<RwLock<HashMap<String, String>>>, // token -> device_id
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            pairing_codes: Arc::new(RwLock::new(HashMap::new())),
            devices: Arc::new(RwLock::new(HashMap::new())),
            tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn create_pairing_code(&self, default_permission: DevicePermission) -> String {
        let code: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

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

        if pairing.created_at.elapsed() > PAIRING_EXPIRY {
            return Err(AuthError::ExpiredPairingCode);
        }

        let device_id = uuid::Uuid::new_v4().to_string();
        let token: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

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

        Ok((token, info))
    }

    pub fn validate_token(&self, token: &str) -> Result<DeviceInfo, AuthError> {
        let device_id = {
            let tokens = self.tokens.read();
            tokens.get(token).cloned()
        }
        .ok_or(AuthError::Unauthorized)?;

        let mut devices = self.devices.write();
        let device = devices.get_mut(&device_id).ok_or(AuthError::Unauthorized)?;

        if device.revoked {
            return Err(AuthError::RevokedDevice);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        device.last_seen_at = now;

        Ok(device.clone())
    }

    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.devices.read().values().cloned().collect()
    }

    pub fn revoke_device(&self, device_id: &str) -> bool {
        let mut devices = self.devices.write();
        if let Some(device) = devices.get_mut(device_id) {
            device.revoked = true;
            true
        } else {
            false
        }
    }
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
