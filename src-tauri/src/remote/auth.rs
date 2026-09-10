use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use parking_lot::{Mutex, MutexGuard, RwLock};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineIdentity {
    pub machine_id: String,
    pub display_name: String,
    /// Standard base64 encoded 32-byte verifying key.
    pub public_key: String,
    /// Standard base64 encoded 32-byte signing key seed; never send to the relay.
    pub private_key: String,
}

pub(crate) fn canonical_identity_dir() -> Result<PathBuf, String> {
    if let Some(base) = std::env::var_os("FERRYX_DATA_DIR") {
        return Ok(PathBuf::from(base));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|base| base.join(".ferryx/remote"))
        .ok_or_else(|| "Cannot resolve machine identity directory".to_string())
}

pub(crate) fn canonical_auth_path() -> Option<PathBuf> {
    std::env::var_os("FERRYX_DATA_DIR")
        .map(|base| PathBuf::from(base).join("remote"))
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|base| PathBuf::from(base).join(".ferryx/remote"))
        })
        .map(|base| base.join("remote-auth.json"))
}

pub fn load_or_generate_machine_identity(base_dir: &Path) -> Result<MachineIdentity, String> {
    let path = base_dir.join("identity.json");
    match std::fs::read(&path) {
        Ok(bytes) => {
            return serde_json::from_slice(&bytes)
                .map_err(|error| format!("Failed to parse machine identity: {error}"));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to read machine identity: {error}")),
    }

    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    let display_name = std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Ferryx machine".to_string());
    let identity = MachineIdentity {
        machine_id: uuid::Uuid::new_v4().to_string(),
        display_name,
        public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        private_key: STANDARD.encode(key.to_bytes()),
    };
    write_private_json(&path, &identity)
        .map_err(|error| format!("Failed to persist machine identity: {error}"))?;
    Ok(identity)
}

pub fn sign_challenge(
    identity: &MachineIdentity,
    nonce: &str,
    timestamp: u64,
) -> Result<String, String> {
    sign_message(identity, &format!("{nonce}:{timestamp}"))
}

pub fn sign_control_challenge(
    identity: &MachineIdentity,
    audience: &str,
    nonce: &str,
    timestamp: u64,
) -> Result<String, String> {
    sign_message(
        identity,
        &format!(
            "ferryx-control-v1:{}:{audience}:{nonce}:{timestamp}",
            identity.machine_id
        ),
    )
}

pub fn verify_control_challenge(
    public_key: &str,
    machine_id: &str,
    audience: &str,
    nonce: &str,
    timestamp: u64,
    signature: &str,
) -> bool {
    verify_message(
        public_key,
        &format!("ferryx-control-v1:{machine_id}:{audience}:{nonce}:{timestamp}"),
        signature,
    )
}

fn sign_message(identity: &MachineIdentity, message: &str) -> Result<String, String> {
    let seed: [u8; 32] = STANDARD
        .decode(&identity.private_key)
        .map_err(|error| format!("Invalid machine private key encoding: {error}"))?
        .try_into()
        .map_err(|_| "Machine private key must contain 32 bytes".to_string())?;
    let signature = SigningKey::from_bytes(&seed).sign(message.as_bytes());
    Ok(STANDARD.encode(signature.to_bytes()))
}

pub fn verify_machine_signature(
    public_key_b64: &str,
    nonce: &str,
    timestamp: u64,
    signature_b64: &str,
) -> bool {
    verify_message(
        public_key_b64,
        &format!("{nonce}:{timestamp}"),
        signature_b64,
    )
}

/// True when `public_key_b64` is a usable base64 Ed25519 verifying key. Used to reject
/// a persisted ownership record that could never authenticate anything.
pub(crate) fn is_valid_public_key(public_key_b64: &str) -> bool {
    STANDARD
        .decode(public_key_b64)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .is_some_and(|bytes| VerifyingKey::from_bytes(&bytes).is_ok())
}

fn verify_message(public_key_b64: &str, message: &str, signature_b64: &str) -> bool {
    let Ok(bytes) = STANDARD.decode(public_key_b64) else {
        return false;
    };
    let Ok(bytes) = <[u8; 32]>::try_from(bytes) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&bytes) else {
        return false;
    };
    let Ok(bytes) = STANDARD.decode(signature_b64) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&bytes) else {
        return false;
    };
    key.verify_strict(message.as_bytes(), &signature).is_ok()
}

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

#[derive(Clone, Serialize, Deserialize)]
struct PairingCode {
    _code: String,
    #[serde(with = "persisted_instant")]
    created_at: Instant,
    default_permission: DevicePermission,
    #[serde(default)]
    approved_token: Option<String>,
}

// Preserve monotonic expiry in-process while storing portable wall-clock timestamps.
mod persisted_instant {
    use super::*;

    pub fn serialize<S: serde::Serializer>(
        instant: &Instant,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let created = std::time::SystemTime::now()
            .checked_sub(instant.elapsed())
            .unwrap_or(std::time::UNIX_EPOCH)
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        created.serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Instant, D::Error> {
        let created = Duration::deserialize(deserializer)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let age = now.checked_sub(created).unwrap_or(PAIRING_EXPIRY);
        Ok(Instant::now() - age.min(PAIRING_EXPIRY))
    }
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
    #[serde(default)]
    pairing_codes: HashMap<String, PairingCode>,
}

#[derive(Clone)]
pub struct AuthManager {
    pairing_window: Arc<RwLock<PairingWindow>>,
    transaction: Arc<Mutex<()>>,
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
            pairing_window: Arc::new(RwLock::new(PairingWindow {
                codes: persisted.pairing_codes,
                ..PairingWindow::default()
            })),
            transaction: Arc::new(Mutex::new(())),
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
        let _transaction = self.begin_transaction();
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
                approved_token: None,
            },
        );
        drop(window);
        self.persist_best_effort();
        code
    }

    /// Installs the relay capability in the same single-use authority as local PINs.
    pub(crate) fn register_pairing_capability(&self, token: &str) {
        let _transaction = self.begin_transaction();
        let mut window = self.pairing_window.write();
        window.refresh(Instant::now());
        window.codes.insert(
            token.to_owned(),
            PairingCode {
                _code: token.to_owned(),
                created_at: Instant::now(),
                default_permission: DevicePermission::Control,
                approved_token: None,
            },
        );
        drop(window);
        self.persist_best_effort();
    }

    pub(crate) fn cancel_pairing_capability(&self, token: &str) {
        let _transaction = self.begin_transaction();
        self.pairing_window.write().codes.remove(token);
        self.persist_best_effort();
    }

    pub fn exchange_pairing_code(
        &self,
        code: &str,
        device_name: &str,
    ) -> Result<(String, DeviceInfo), AuthError> {
        let _transaction = self.begin_transaction();
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

        if let Some(token) = pairing.approved_token {
            let device_id = self.tokens.read().get(&token).cloned();
            let info = device_id.and_then(|id| self.devices.read().get(&id).cloned());
            self.persist_best_effort();
            return info
                .map(|info| (token, info))
                .ok_or(AuthError::Unauthorized);
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

    /// Approves a pairing PIN from a headless/CLI context (e.g. `ferryx pair approve <pin>`).
    /// Persists an approved device and its bearer token with the PIN so a remote
    /// client can retrieve that same token in a single-use exchange.
    pub fn approve_pairing_code_cli(&self, code: &str) -> Result<DeviceInfo, AuthError> {
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err(AuthError::InvalidPairingCode);
        }

        let _transaction = self.begin_transaction();
        let mut pairing = {
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

        if let Some(token) = &pairing.approved_token {
            let device_id = self.tokens.read().get(token).cloned();
            let info = device_id.and_then(|id| self.devices.read().get(&id).cloned());
            self.pairing_window
                .write()
                .codes
                .insert(code.to_string(), pairing);
            return info.ok_or(AuthError::Unauthorized);
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
            name: "cli-paired-device".to_string(),
            permission: pairing.default_permission,
            created_at: now,
            last_seen_at: now,
            revoked: false,
        };

        self.devices.write().insert(device_id.clone(), info.clone());
        self.tokens.write().insert(token.clone(), device_id);
        pairing.approved_token = Some(token);
        self.pairing_window
            .write()
            .codes
            .insert(code.to_string(), pairing);
        self.persist_best_effort();
        Ok(info)
    }

    pub fn validate_token(&self, token: &str) -> Result<DeviceInfo, AuthError> {
        let _transaction = self.begin_transaction();
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
        let _transaction = self.begin_transaction();
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

    // A separate lock file survives atomic replacement of the JSON inode.
    // Serialize reload/mutation/save across both clones and independent processes.
    fn begin_transaction(&self) -> (MutexGuard<'_, ()>, Option<std::fs::File>) {
        let guard = self.transaction.lock();
        let file = self.persistence_path.as_deref().map(|path| {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create remote auth directory");
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path.with_extension("lock"))
                .expect("open remote auth lock");
            file.lock().expect("lock remote auth state");
            if let Some(mut state) = load_persisted_auth(path) {
                prune_revoked_devices(&mut state);
                self.pairing_window.write().codes = state.pairing_codes;
                *self.devices.write() = state.devices;
                *self.tokens.write() = state.tokens;
            }
            file
        });
        (guard, file)
    }

    fn persist_best_effort(&self) {
        let Some(path) = self.persistence_path.as_deref() else {
            return;
        };
        *self.last_persisted_at.write() = Instant::now();
        let snapshot = PersistedAuthState {
            devices: self.devices.read().clone(),
            tokens: self.tokens.read().clone(),
            pairing_codes: self.pairing_window.read().codes.clone(),
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
    let PersistedAuthState {
        devices, tokens, ..
    } = state;
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
    fn test_machine_identity_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("machine");
        let identity = load_or_generate_machine_identity(&base).unwrap();
        assert!(base.join("identity.json").is_file());
        assert!(uuid::Uuid::parse_str(&identity.machine_id).is_ok());
        assert!(!identity.display_name.is_empty());
        assert_eq!(STANDARD.decode(&identity.public_key).unwrap().len(), 32);
        assert_eq!(STANDARD.decode(&identity.private_key).unwrap().len(), 32);
        let reloaded = load_or_generate_machine_identity(&base).unwrap();
        assert_eq!(identity.machine_id, reloaded.machine_id);
        assert_eq!(identity.display_name, reloaded.display_name);
        assert_eq!(identity.public_key, reloaded.public_key);
        assert_eq!(identity.private_key, reloaded.private_key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(base.join("identity.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(base.join("identity.json"), b"invalid json").unwrap();
        assert!(load_or_generate_machine_identity(&base).is_err());
    }

    #[test]
    fn test_machine_identity_sign_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let identity = load_or_generate_machine_identity(dir.path()).unwrap();
        let nonce = "challenge-nonce";
        let timestamp = 1_700_000_000;
        let signature = sign_challenge(&identity, nonce, timestamp).unwrap();
        assert!(verify_machine_signature(
            &identity.public_key,
            nonce,
            timestamp,
            &signature
        ));
        assert!(!verify_machine_signature(
            &identity.public_key,
            "wrong-nonce",
            timestamp,
            &signature
        ));
        assert!(!verify_machine_signature(
            &identity.public_key,
            nonce,
            timestamp + 1,
            &signature
        ));
        assert!(!verify_machine_signature(
            &identity.public_key,
            nonce,
            timestamp,
            "invalid!"
        ));
        assert!(!verify_machine_signature(
            &identity.public_key,
            nonce,
            timestamp,
            &STANDARD.encode([0u8; 64])
        ));
        assert!(!verify_machine_signature(
            "invalid!", nonce, timestamp, &signature
        ));
        assert!(!verify_machine_signature(
            &STANDARD.encode([0u8; 31]),
            nonce,
            timestamp,
            &signature
        ));
        let mut invalid = identity.clone();
        invalid.private_key = "invalid!".into();
        assert!(sign_challenge(&invalid, nonce, timestamp).is_err());
        invalid.private_key = STANDARD.encode([0u8; 31]);
        assert!(sign_challenge(&invalid, nonce, timestamp).is_err());
    }

    #[test]
    fn relay_capability_is_single_use_persisted_and_cancellable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote-auth.json");
        let gateway = AuthManager::with_persistence(Some(path.clone()));
        let coordinator = AuthManager::with_persistence(Some(path));
        coordinator.register_pairing_capability("capability");
        let (token, device) = gateway
            .exchange_pairing_code("capability", "browser")
            .unwrap();
        assert_eq!(gateway.validate_token(&token).unwrap().id, device.id);
        assert!(gateway
            .exchange_pairing_code("capability", "replay")
            .is_err());
        coordinator.register_pairing_capability("cancelled");
        coordinator.cancel_pairing_capability("cancelled");
        assert!(gateway
            .exchange_pairing_code("cancelled", "browser")
            .is_err());
    }

    #[test]
    fn test_cli_pair_cross_process_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote-auth.json");
        let gateway = AuthManager::with_persistence(Some(path.clone()));
        let generator = AuthManager::with_persistence(Some(path.clone()));
        let code = generator.create_pairing_code(DevicePermission::View);
        drop(generator);
        let approver = AuthManager::with_persistence(Some(path.clone()));
        let approved = approver.approve_pairing_code_cli(&code).unwrap();
        let issued_token = load_persisted_auth(&path).unwrap().pairing_codes[&code]
            .approved_token
            .clone()
            .unwrap();
        assert_eq!(
            approver.approve_pairing_code_cli(&code).unwrap().id,
            approved.id
        );
        drop(approver);
        let (token, device) = gateway.exchange_pairing_code(&code, "Phone").unwrap();
        assert_eq!(token, issued_token);
        assert_eq!(device.id, approved.id);
        assert_eq!(device.permission, DevicePermission::View);
        assert_eq!(gateway.validate_token(&token).unwrap().id, approved.id);
        let reopened = AuthManager::with_persistence(Some(path.clone()));
        assert!(matches!(
            reopened.exchange_pairing_code(&code, "Replay"),
            Err(AuthError::InvalidPairingCode)
        ));
        assert_eq!(reopened.validate_token(&token).unwrap().id, approved.id);
        let expired = gateway.create_pairing_code(DevicePermission::Control);
        let mut state = load_persisted_auth(&path).unwrap();
        state.pairing_codes.get_mut(&expired).unwrap().created_at = Instant::now() - PAIRING_EXPIRY;
        write_private_json(&path, &state).unwrap();
        assert!(matches!(
            reopened.approve_pairing_code_cli(&expired),
            Err(AuthError::ExpiredPairingCode)
        ));
        assert!(gateway.exchange_pairing_code(&expired, "Expired").is_err());
    }

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
