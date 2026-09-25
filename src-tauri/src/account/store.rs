use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const LOGIN_CODE_TTL: Duration = Duration::from_secs(600);
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const ENROLLMENT_CODE_TTL: Duration = Duration::from_secs(600);
pub const DEFAULT_LOGIN_REQUESTS_PER_HOUR: u32 = 5;
pub const DEFAULT_MAX_BODY_BYTES: usize = 4096;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

pub fn token_hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserRecord {
    pub user_id: String,
    pub email: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub user_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoginCodeRecord {
    pub email: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentCodeRecord {
    pub user_id: String,
    pub account_origin: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MachineRecord {
    pub machine_record_id: String,
    pub owner_user_id: String,
    pub machine_id: String,
    pub display_name: String,
    pub public_key: String,
    pub attach_public_key: String,
    pub relay_origin: String,
    pub platform: String,
    pub enrollment_epoch: u64,
    pub enrolled_at: u64,
    #[serde(default)]
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrantRecord {
    pub grant_id: String,
    pub machine_record_id: String,
    pub owner_user_id: String,
    pub pairing_token_hash: String,
    pub grant_scope: String,
    pub device_attach_public_key: String,
    pub installation_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthRecord {
    pub device_code_hash: String,
    pub user_code: String,
    pub email: String,
    pub email_token_hash: String,
    pub enrollment_code: Option<String>,
    pub expires_at: u64,
}

pub const ACCOUNT_SIGNING_KEY_FILE: &str = "account-signing-key.json";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSigningKeyRecord {
    pub public_key: String,
    pub private_key: String,
}

impl std::fmt::Debug for AccountSigningKeyRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountSigningKeyRecord")
            .field("public_key", &self.public_key)
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountStore {
    #[serde(default)]
    pub users: BTreeMap<String, UserRecord>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionRecord>,
    #[serde(default)]
    pub login_codes: BTreeMap<String, LoginCodeRecord>,
    #[serde(default)]
    pub enrollment_codes: BTreeMap<String, EnrollmentCodeRecord>,
    #[serde(default)]
    pub machines: BTreeMap<String, MachineRecord>,
    #[serde(default)]
    pub grants: BTreeMap<String, GrantRecord>,
    #[serde(default)]
    pub device_auths: BTreeMap<String, DeviceAuthRecord>,
}

impl AccountStore {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = store_path(dir);
        match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|error| format!("STORE_CORRUPT: {error}"))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("Failed to read account store: {error}")),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = store_path(dir);
        write_private_json(&path, self).map_err(|error| format!("Failed to persist: {error}"))
    }

    pub fn purge_expired(&mut self, now: u64) {
        self.enrollment_codes
            .retain(|_, code| code.expires_at > now);
        self.sessions.retain(|_, session| session.expires_at > now);
        self.grants.retain(|_, grant| grant.expires_at > now);
        self.device_auths.retain(|_, auth| auth.expires_at > now);
    }

    pub fn user_by_email(&self, email: &str) -> Option<&UserRecord> {
        self.users.values().find(|user| user.email == email)
    }

    pub fn user_for_session(&self, bearer: &str, now: u64) -> Option<&UserRecord> {
        let session = self.sessions.get(&token_hash(bearer))?;
        if session.expires_at <= now {
            return None;
        }
        self.users.get(&session.user_id)
    }
}

pub fn store_path(dir: &Path) -> PathBuf {
    dir.join("account-store.json")
}

pub fn signing_key_path(dir: &Path) -> PathBuf {
    dir.join(ACCOUNT_SIGNING_KEY_FILE)
}

pub fn lock_account_dir(dir: &Path) -> Result<File, String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("Failed to create account data dir: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Failed to restrict account data dir: {error}"))?;
    }
    let path = dir.join("account-store.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| format!("Failed to open account lock: {error}"))?;
    file.lock()
        .map_err(|error| format!("Failed to lock account store: {error}"))?;
    Ok(file)
}

pub(crate) fn write_private_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        use std::io::Write;
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_store_round_trips_without_secrets() {
        let dir = tempfile::tempdir().expect("temp");
        let mut store = AccountStore::default();
        let token = random_token();
        store
            .login_codes
            .insert(token_hash(&token), LoginCodeRecord {
                email: "a@b.co".into(),
                expires_at: now_secs() + 600,
            });
        store.save(dir.path()).expect("save");
        let raw = std::fs::read_to_string(store_path(dir.path())).expect("raw");
        assert!(!raw.contains(&token), "raw login token must not be persisted");
        let loaded = AccountStore::load(dir.path()).expect("load");
        assert_eq!(loaded.login_codes.len(), 1);

        store.login_codes.clear();
        store.login_codes.insert(token_hash("expired"), LoginCodeRecord {
            email: "a@b.co".into(),
            expires_at: now_secs() - 1,
        });
        store.sessions.insert(
            "session-hash".into(),
            SessionRecord {
                user_id: "u1".into(),
                expires_at: now_secs() - 1,
            },
        );
        store.enrollment_codes.insert(
            "code-hash".into(),
            EnrollmentCodeRecord {
                user_id: "u1".into(),
                account_origin: "https://account.example".into(),
                expires_at: now_secs() - 1,
            },
        );
        store.purge_expired(now_secs());
        assert!(store.sessions.is_empty(), "expired sessions are dropped");
        assert!(store.enrollment_codes.is_empty(), "expired enrollment codes are dropped");
        assert_eq!(
            store.login_codes.len(),
            1,
            "login codes stay so the consume path can report expiry instead of reuse"
        );
    }

    #[test]
    fn account_signing_key_record_debug_redacts_private_key() {
        let record = AccountSigningKeyRecord {
            public_key: "pub".into(),
            private_key: "secret".into(),
        };
        let formatted = format!("{record:?}");
        assert!(!formatted.contains("secret"));
        assert!(formatted.contains("[REDACTED]"));
    }
}
