use parking_lot::RwLock;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PAIRING_EXPIRY: Duration = Duration::from_secs(60);
const PAIRING_FAILURE_BUDGET: u8 = 5;
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
    /// Always `false` for a live device: revoking deletes the device outright.
    /// Retained so stores written by older builds, which tombstoned devices
    /// instead of removing them, can be pruned on load.
    #[serde(default)]
    pub revoked: bool,
}

struct PairingCode {
    _code: String,
    created_at: Instant,
    default_permission: DevicePermission,
}

#[derive(Default)]
struct PairingWindow {
    codes: HashMap<String, PairingCode>,
    started_at: Option<Instant>,
    failures: u8,
}

impl PairingWindow {
    fn refresh(&mut self, now: Instant) {
        if self
            .started_at
            .is_none_or(|start| now.duration_since(start) >= PAIRING_EXPIRY)
        {
            self.codes
                .retain(|_, pairing| now.duration_since(pairing.created_at) < PAIRING_EXPIRY);
            self.started_at = Some(now);
            self.failures = 0;
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedAuthState {
    devices: HashMap<String, DeviceInfo>,
    tokens: HashMap<String, String>,
}

#[derive(Clone)]
pub struct AuthManager {
    pairing_window: Arc<RwLock<PairingWindow>>,
    devices: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    /// Device Bearer Tokens: long-lived credentials issued to paired remote
    /// control clients. Maps token -> owning device id.
    tokens: Arc<RwLock<HashMap<String, String>>>,
    /// Machine Tokens: credentials used by the local daemon to authenticate
    /// its reverse tunnel connection to the relay. Distinct tier from device
    /// bearer tokens; a machine token identifies the host, not a paired
    /// remote-control device, and is never handed out via pairing.
    machine_tokens: Arc<RwLock<std::collections::HashSet<String>>>,
    revocations: Arc<RwLock<HashMap<String, tokio::sync::watch::Sender<bool>>>>,
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
        let mut persisted = persistence_path
            .as_deref()
            .and_then(load_persisted_auth)
            .unwrap_or_default();
        prune_revoked_devices(&mut persisted);
        Self {
            pairing_window: Arc::new(RwLock::new(PairingWindow::default())),
            devices: Arc::new(RwLock::new(persisted.devices)),
            tokens: Arc::new(RwLock::new(persisted.tokens)),
            machine_tokens: Arc::new(RwLock::new(std::collections::HashSet::new())),
            revocations: Arc::new(RwLock::new(HashMap::new())),
            persistence_path,
            last_persisted_at: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Generates a new Machine Token authenticating this daemon's reverse
    /// tunnel connection to the relay. Distinct from Device Bearer Tokens:
    /// it identifies the machine itself, not a paired remote-control device,
    /// and is not subject to pairing-code exchange or device revocation.
    pub fn generate_machine_token(&self) -> String {
        let token: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();
        self.machine_tokens.write().insert(token.clone());
        token
    }

    /// Validates a Machine Token presented by the daemon when establishing
    /// a reverse tunnel to the relay. Never matches Device Bearer Tokens or
    /// Pairing PINs; each credential tier is checked against its own store.
    pub fn validate_machine_token(&self, token: &str) -> Result<(), AuthError> {
        if self.machine_tokens.read().contains(token) {
            Ok(())
        } else {
            Err(AuthError::Unauthorized)
        }
    }

    /// Revokes a previously generated Machine Token so it can no longer
    /// authenticate reverse tunnel connections.
    pub fn revoke_machine_token(&self, token: &str) -> bool {
        self.machine_tokens.write().remove(token)
    }

    pub fn create_pairing_code(&self, default_permission: DevicePermission) -> String {
        let pin: u32 = rand::thread_rng().gen_range(100_000..=999_999);
        let code = format!("{pin:06}");

        let mut window = self.pairing_window.write();
        window.refresh(Instant::now());
        window.codes.insert(
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
        let pairing = {
            // Lookup, failure accounting and single-use consumption share one
            // lock. No concurrent request can spend the same budget slot/code.
            let mut window = self.pairing_window.write();
            window.refresh(Instant::now());
            if window.failures >= PAIRING_FAILURE_BUDGET {
                return Err(AuthError::PairingRateLimited);
            }
            let Some(pairing) = window.codes.remove(code) else {
                window.failures += 1;
                return Err(AuthError::InvalidPairingCode);
            };
            if pairing.created_at.elapsed() >= PAIRING_EXPIRY {
                window.failures += 1;
                return Err(AuthError::ExpiredPairingCode);
            }
            pairing
        };

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

    /// Approves a pairing PIN from a headless/CLI context (e.g. `ferryx pair approve <pin>`).
    /// Validates the 6-digit numeric code against active pairing codes and registers an
    /// approved CLI device, mirroring [`Self::exchange_pairing_code`] but without issuing a
    /// bearer token, since CLI approval only needs to confirm the device was registered.
    pub fn approve_pairing_code_cli(&self, code: &str) -> Result<DeviceInfo, AuthError> {
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(AuthError::InvalidPairingCode);
        }

        let pairing = {
            let mut window = self.pairing_window.write();
            window.refresh(Instant::now());
            if window.failures >= PAIRING_FAILURE_BUDGET {
                return Err(AuthError::PairingRateLimited);
            }
            let Some(pairing) = window.codes.remove(code) else {
                window.failures += 1;
                return Err(AuthError::InvalidPairingCode);
            };
            if pairing.created_at.elapsed() >= PAIRING_EXPIRY {
                window.failures += 1;
                return Err(AuthError::ExpiredPairingCode);
            }
            pairing
        };

        let device_id = uuid::Uuid::new_v4().to_string();
        let token: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        let now = unix_now();
        let info = DeviceInfo {
            id: device_id.clone(),
            name: "cli-paired-device".to_string(),
            permission: pairing.default_permission,
            created_at: now,
            last_seen_at: now,
            revoked: false,
        };

        self.devices.write().insert(device_id.clone(), info.clone());
        self.tokens.write().insert(token, device_id);
        self.persist_best_effort();
        Ok(info)
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

    /// Registration and revocation both hold the devices lock before the
    /// signal registry lock. A caller validated just before revocation either
    /// gets its latched signal or is rejected here; there is no subscribe gap.
    pub(crate) fn device_revocation(
        &self,
        device_id: &str,
    ) -> Result<tokio::sync::watch::Receiver<bool>, AuthError> {
        let devices = self.devices.read();
        if !devices.contains_key(device_id) {
            return Err(AuthError::Unauthorized);
        }
        let mut revocations = self.revocations.write();
        Ok(revocations
            .entry(device_id.to_owned())
            .or_insert_with(|| tokio::sync::watch::channel(false).0)
            .subscribe())
    }

    /// Deletes the device and every token issued to it. The device disappears
    /// from [`Self::list_devices`] immediately instead of lingering as a
    /// revoked entry.
    pub fn revoke_device(&self, device_id: &str) -> bool {
        let changed = {
            let mut devices = self.devices.write();
            if devices.remove(device_id).is_some() {
                self.tokens.write().retain(|_, owner| owner != device_id);
                if let Some(signal) = self.revocations.write().remove(device_id) {
                    // Retain cancellation even if on_upgrade has not started.
                    signal.send_replace(true);
                }
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

    /// Test-only hook: backdates an active pairing PIN's creation time so
    /// expiration logic can be exercised deterministically, without a real
    /// 60-second sleep in the test suite.
    #[cfg(test)]
    pub(crate) fn backdate_pairing_code(&self, code: &str, age: Duration) {
        let mut window = self.pairing_window.write();
        if let Some(pairing) = window.codes.get_mut(code) {
            pairing.created_at = Instant::now()
                .checked_sub(age)
                .expect("instant subtraction");
        }
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

/// Drops devices that older builds tombstoned with `revoked: true`, together
/// with their tokens, so a revoked device never resurfaces in the device list.
fn prune_revoked_devices(state: &mut PersistedAuthState) {
    let PersistedAuthState { devices, tokens } = state;
    devices.retain(|_, device| !device.revoked);
    tokens.retain(|_, device_id| devices.contains_key(device_id));
}

/// Writes `value` as JSON, restricting access to the current user.
///
/// On Unix the owner-only modes are applied explicitly, and the file is written to a temporary
/// path first so it is never briefly visible with the default umask under its final name.
/// On Windows the enclosing per-user directory (`LOCALAPPDATA`, chosen by
/// `remote::state::resolve_remote_data_dir`) already carries an ACL that excludes other standard
/// users, and both the temporary and final file inherit it.
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
    #[error("Pairing attempt limit reached; wait for a new pairing window")]
    PairingRateLimited,
    #[error("Unauthorized access")]
    Unauthorized,
}

#[cfg(test)]
#[path = "auth_security_tests.rs"]
mod security_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_pair_approve() {
        let manager = AuthManager::new();
        let code = manager.create_pairing_code(DevicePermission::Control);

        let device = manager
            .approve_pairing_code_cli(&code)
            .expect("approving a freshly created pairing code must succeed");
        assert_eq!(device.name, "cli-paired-device");
        assert_eq!(device.permission, DevicePermission::Control);
        assert_eq!(manager.list_devices().len(), 1);

        let invalid = manager.approve_pairing_code_cli("000000");
        assert!(matches!(invalid, Err(AuthError::InvalidPairingCode)));
    }

    /// Verifies the three credential tiers - Machine Token, 60s Pairing PIN,
    /// and Device Bearer Token - are validated independently, that a token
    /// from one tier never authenticates another, and that the pairing PIN
    /// both expires after 60 seconds and is consumed on first use.
    #[test]
    fn test_auth_credential_separation() {
        let manager = AuthManager::new();

        // --- Tier 1: Machine Token (daemon-to-relay reverse tunnel) ---
        let machine_token = manager.generate_machine_token();
        assert!(
            manager.validate_machine_token(&machine_token).is_ok(),
            "a freshly generated machine token must validate"
        );
        assert!(matches!(
            manager.validate_machine_token("not-a-real-machine-token"),
            Err(AuthError::Unauthorized)
        ));

        // --- Tier 2: 60-second Pairing PIN, single-use, strict expiry ---
        let code = manager.create_pairing_code(DevicePermission::Control);
        assert_eq!(code.len(), 6, "pairing PIN must be a 6-digit code");

        // A pairing PIN must never validate as a machine token or vice versa.
        assert!(matches!(
            manager.validate_machine_token(&code),
            Err(AuthError::Unauthorized)
        ));

        // Backdate the PIN past its 60-second window and confirm it is rejected.
        manager.backdate_pairing_code(&code, Duration::from_secs(61));
        assert!(matches!(
            manager.exchange_pairing_code(&code, "Expired Phone"),
            Err(AuthError::ExpiredPairingCode)
        ));

        // Issue a fresh PIN and consume it exactly once.
        let code = manager.create_pairing_code(DevicePermission::View);
        let (device_token, device) = manager
            .exchange_pairing_code(&code, "Tablet")
            .expect("a fresh, unexpired pairing code must exchange successfully");
        assert_eq!(device.permission, DevicePermission::View);

        // Re-using the same PIN must fail: pairing codes are single-use.
        assert!(matches!(
            manager.exchange_pairing_code(&code, "Second Device"),
            Err(AuthError::InvalidPairingCode)
        ));

        // --- Tier 3: Device Bearer Token ---
        let validated = manager
            .validate_token(&device_token)
            .expect("a token minted by pairing exchange must validate as a device bearer token");
        assert_eq!(validated.id, device.id);

        // A device bearer token must never validate as a machine token.
        assert!(matches!(
            manager.validate_machine_token(&device_token),
            Err(AuthError::Unauthorized)
        ));

        // A machine token must never validate as a device bearer token.
        assert!(matches!(
            manager.validate_token(&machine_token),
            Err(AuthError::Unauthorized)
        ));

        // Revoking the machine token removes it from the machine-token tier only.
        assert!(manager.revoke_machine_token(&machine_token));
        assert!(matches!(
            manager.validate_machine_token(&machine_token),
            Err(AuthError::Unauthorized)
        ));
        assert!(
            manager.validate_token(&device_token).is_ok(),
            "revoking a machine token must not affect device bearer tokens"
        );
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn revoked_devices_are_deleted_outright_and_never_reappear() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("remote-auth.json");
        let manager = AuthManager::with_persistence(Some(path.clone()));
        let code = manager.create_pairing_code(DevicePermission::Control);
        let (token, device) = manager.exchange_pairing_code(&code, "Phone").expect("pair");
        assert!(manager.revoke_device(&device.id));

        assert!(
            manager.list_devices().is_empty(),
            "revoking must delete the device from the list, not leave a tombstone"
        );
        assert!(matches!(
            manager.validate_token(&token),
            Err(AuthError::Unauthorized)
        ));

        let reopened = AuthManager::with_persistence(Some(path.clone()));
        assert!(
            reopened.list_devices().is_empty(),
            "a deleted device must not come back after reopen"
        );
        assert!(matches!(
            reopened.validate_token(&token),
            Err(AuthError::Unauthorized)
        ));

        let persisted = std::fs::read_to_string(&path).expect("read persisted state");
        assert!(
            !persisted.contains(&device.id),
            "the deleted device must not linger on disk: {persisted}"
        );
        assert!(
            !persisted.contains(&token),
            "the deleted device's token must not linger on disk: {persisted}"
        );
    }

    #[test]
    fn legacy_revoked_tombstones_are_pruned_when_the_store_is_loaded() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("remote-auth.json");
        let legacy = serde_json::json!({
            "devices": {
                "dead-id": {
                    "id": "dead-id",
                    "name": "Old Phone",
                    "permission": "control",
                    "createdAt": 1,
                    "lastSeenAt": 2,
                    "revoked": true
                },
                "live-id": {
                    "id": "live-id",
                    "name": "Current Phone",
                    "permission": "control",
                    "createdAt": 3,
                    "lastSeenAt": 4,
                    "revoked": false
                }
            },
            "tokens": { "dead-token": "dead-id", "live-token": "live-id" }
        });
        std::fs::write(
            &path,
            serde_json::to_vec(&legacy).expect("serialize legacy state"),
        )
        .expect("write legacy state");

        let manager = AuthManager::with_persistence(Some(path));
        let listed = manager.list_devices();
        assert_eq!(
            listed.len(),
            1,
            "pre-existing revoked tombstones must be dropped on load"
        );
        assert_eq!(listed[0].id, "live-id");
        assert!(matches!(
            manager.validate_token("dead-token"),
            Err(AuthError::Unauthorized)
        ));
        assert_eq!(
            manager.validate_token("live-token").expect("live token").id,
            "live-id"
        );
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
