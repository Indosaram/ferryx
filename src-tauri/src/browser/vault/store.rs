//! The durable encrypted vault: `<app data>/browser-vault/vault.json`.
//!
//! Each login is one AES-256-GCM record (see `crypto`) whose plaintext is its username and
//! password; the record is bound by AAD to its item id and origin, so a record copied onto
//! another item or origin fails to decrypt. The 256-bit key lives only in the OS keystore,
//! under the store's random `keyId`. The origin is public metadata so matching needs no
//! decryption. Writes are atomic (temp file + rename). A store that cannot be parsed or whose
//! records do not decrypt with the keystore's key is reported as corrupted and never rewritten.
//!
//! Record payload and item shape follow Maho's vault_manager/record.rs and vault_items
//! (Project-Maho/maho-browser@c2830251), reduced to logins.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::crypto::{
    decrypt_record, encrypt_record, VaultCiphertextEnvelope, VaultCiphertextPurpose, VaultCryptoError,
    VaultKey, VaultKeyVersion,
};
use super::keystore::{KeyStore, KeystoreError};
use super::origin::{canonicalize_origin, origin_of_page, CredentialOrigin};

pub const STORE_FILE: &str = "vault.json";
const STORE_VERSION: u32 = 1;
const MAX_USERNAME_LEN: usize = 512;
const MAX_PASSWORD_LEN: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("password storage is unavailable: {0}")]
    Unavailable(String),
    #[error("the OS keystore failed: {0}")]
    Keystore(String),
    #[error("the password vault is damaged: {0}")]
    Corrupted(String),
    #[error("no saved password with id {0}")]
    NotFound(String),
    #[error("this save prompt is no longer open")]
    PromptExpired,
    #[error("{0}")]
    InvalidArgument(String),
    #[error("password vault I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

impl From<KeystoreError> for VaultError {
    fn from(error: KeystoreError) -> Self {
        match error {
            KeystoreError::Unavailable(reason) => Self::Unavailable(reason),
            KeystoreError::Failed(reason) => Self::Keystore(reason),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredItem {
    id: String,
    origin: CredentialOrigin,
    created_at: i64,
    updated_at: i64,
    record: VaultCiphertextEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoreFile {
    version: u32,
    key_id: String,
    key_version: VaultKeyVersion,
    items: Vec<StoredItem>,
}

/// The encrypted part of a login.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
struct LoginPayload {
    username: String,
    password: String,
}

/// What the UI may see of a login: never the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultItemSummary {
    pub id: String,
    pub origin: String,
    pub username: String,
    /// Milliseconds since the Unix epoch.
    pub created_at: i64,
    pub updated_at: i64,
}

/// A login with its secret, only for filling a page or a revealed copy.
pub struct VaultCredential {
    pub origin: CredentialOrigin,
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
}

/// What saving a submitted login would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveKind {
    /// The same username and password are already saved for this origin.
    Unchanged,
    New,
    /// The username is saved for this origin with another password.
    Update { id: String },
}

pub struct VaultStore {
    path: PathBuf,
    file: StoreFile,
    key: VaultKey,
    /// Decrypted usernames, by item id (passwords are decrypted on demand only).
    usernames: std::collections::HashMap<String, String>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

fn aad(id: &str, origin: &CredentialOrigin) -> Vec<u8> {
    format!("ferryx-vault-login-v1:{id}:{}", origin.as_str()).into_bytes()
}

fn corrupted(error: VaultCryptoError) -> VaultError {
    VaultError::Corrupted(format!(
        "a saved login does not decrypt with the key in the OS keystore ({error})"
    ))
}

fn validate(username: &str, password: &str) -> Result<(), VaultError> {
    if password.is_empty() {
        return Err(VaultError::InvalidArgument("the password is empty".into()));
    }
    if username.len() > MAX_USERNAME_LEN || password.len() > MAX_PASSWORD_LEN {
        return Err(VaultError::InvalidArgument("the username or password is too long".into()));
    }
    Ok(())
}

impl VaultStore {
    /// Opens the store in `dir`, creating it and its key on first use. Blocking.
    pub fn open(dir: &Path, keystore: &dyn KeyStore) -> Result<Self, VaultError> {
        let path = dir.join(STORE_FILE);
        let file = match std::fs::read(&path) {
            Ok(bytes) => Some(serde_json::from_slice::<StoreFile>(&bytes).map_err(|error| {
                VaultError::Corrupted(format!("{} cannot be read: {error}", path.display()))
            })?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let (file, key) = match file {
            Some(file) => {
                if file.version != STORE_VERSION {
                    return Err(VaultError::Corrupted(format!(
                        "unsupported vault store version {}",
                        file.version
                    )));
                }
                let key = match keystore.load(&file.key_id)? {
                    Some(bytes) => {
                        let bytes = Zeroizing::new(bytes);
                        VaultKey::from_slice(&bytes, file.key_version).ok_or_else(|| {
                            VaultError::Corrupted("the OS keystore holds a malformed vault key".into())
                        })?
                    }
                    None if file.items.is_empty() => {
                        let key = VaultKey::generate(file.key_version).map_err(corrupted)?;
                        keystore.store(&file.key_id, key.expose())?;
                        key
                    }
                    None => {
                        return Err(VaultError::Corrupted(
                            "the vault key is missing from the OS keystore, so saved passwords cannot be read".into(),
                        ))
                    }
                };
                (file, key)
            }
            None => {
                let file = StoreFile {
                    version: STORE_VERSION,
                    key_id: uuid::Uuid::new_v4().to_string(),
                    key_version: VaultKeyVersion::new(1),
                    items: Vec::new(),
                };
                let key = VaultKey::generate(file.key_version).map_err(corrupted)?;
                keystore.store(&file.key_id, key.expose())?;
                (file, key)
            }
        };
        let mut store = Self { path, file, key, usernames: Default::default() };
        // Every record must decrypt with this key before the store is used or rewritten.
        for item in store.file.items.clone() {
            let payload = store.payload(&item)?;
            store.usernames.insert(item.id.clone(), payload.username.clone());
        }
        if !store.path.exists() {
            store.persist()?;
        }
        Ok(store)
    }

    fn payload(&self, item: &StoredItem) -> Result<LoginPayload, VaultError> {
        let plaintext = decrypt_record(&item.record, &self.key, &aad(&item.id, &item.origin))
            .map_err(corrupted)?;
        serde_json::from_slice(&plaintext)
            .map_err(|error| VaultError::Corrupted(format!("a saved login is malformed: {error}")))
    }

    fn seal(&self, id: &str, origin: &CredentialOrigin, payload: &LoginPayload) -> Result<VaultCiphertextEnvelope, VaultError> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(payload).map_err(|error| VaultError::InvalidArgument(error.to_string()))?,
        );
        encrypt_record(&plaintext, &self.key, VaultCiphertextPurpose::VaultItem, &aad(id, origin))
            .map_err(|error| VaultError::InvalidArgument(error.to_string()))
    }

    fn persist(&self) -> Result<(), VaultError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(&self.file)
            .map_err(|error| VaultError::InvalidArgument(error.to_string()))?;
        let staging = self.path.with_extension("json.tmp");
        std::fs::write(&staging, bytes)?;
        std::fs::rename(&staging, &self.path)?;
        Ok(())
    }

    fn summary(&self, item: &StoredItem) -> VaultItemSummary {
        VaultItemSummary {
            id: item.id.clone(),
            origin: item.origin.as_str().to_string(),
            username: self.usernames.get(&item.id).cloned().unwrap_or_default(),
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }

    fn item(&self, id: &str) -> Result<&StoredItem, VaultError> {
        self.file.items.iter().find(|item| item.id == id).ok_or_else(|| VaultError::NotFound(id.to_string()))
    }

    /// Every login, newest first, optionally filtered by a case-insensitive origin/username match.
    pub fn list(&self, query: Option<&str>) -> Vec<VaultItemSummary> {
        let query = query.map(str::trim).filter(|query| !query.is_empty()).map(str::to_lowercase);
        let mut items: Vec<VaultItemSummary> = self
            .file
            .items
            .iter()
            .map(|item| self.summary(item))
            .filter(|summary| {
                query.as_ref().is_none_or(|query| {
                    summary.origin.to_lowercase().contains(query) || summary.username.to_lowercase().contains(query)
                })
            })
            .collect();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.id.cmp(&b.id)));
        items
    }

    /// Logins saved for exactly the origin of `page_url` (scheme, host and port).
    pub fn matches(&self, page_url: &str) -> Vec<VaultItemSummary> {
        let Ok(origin) = origin_of_page(page_url) else {
            return Vec::new();
        };
        let mut items: Vec<VaultItemSummary> = self
            .file
            .items
            .iter()
            .filter(|item| item.origin == origin)
            .map(|item| self.summary(item))
            .collect();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.id.cmp(&b.id)));
        items
    }

    pub fn credential(&self, id: &str) -> Result<VaultCredential, VaultError> {
        let item = self.item(id)?;
        let payload = self.payload(item)?;
        Ok(VaultCredential {
            origin: item.origin.clone(),
            username: Zeroizing::new(payload.username.clone()),
            password: Zeroizing::new(payload.password.clone()),
        })
    }

    /// What saving `username`/`password` for `origin` would do.
    pub fn classify(&self, origin: &CredentialOrigin, username: &str, password: &str) -> Result<SaveKind, VaultError> {
        for item in self.file.items.iter().filter(|item| &item.origin == origin) {
            let payload = self.payload(item)?;
            if payload.username == username {
                return Ok(if payload.password == password {
                    SaveKind::Unchanged
                } else {
                    SaveKind::Update { id: item.id.clone() }
                });
            }
        }
        Ok(SaveKind::New)
    }

    /// Saves a login, replacing the password of the same username on the same origin.
    pub fn save(&mut self, origin: &str, username: &str, password: &str) -> Result<VaultItemSummary, VaultError> {
        validate(username, password)?;
        let origin = canonicalize_origin(origin).map_err(|error| VaultError::InvalidArgument(error.to_string()))?;
        match self.classify(&origin, username, password)? {
            SaveKind::Update { id } => self.update(&id, username, Some(password)),
            SaveKind::Unchanged => {
                let id = self
                    .file
                    .items
                    .iter()
                    .find(|item| item.origin == origin && self.usernames.get(&item.id).map(String::as_str) == Some(username))
                    .map(|item| item.id.clone())
                    .ok_or_else(|| VaultError::NotFound(username.to_string()))?;
                Ok(self.summary(self.item(&id)?))
            }
            SaveKind::New => {
                let id = uuid::Uuid::new_v4().to_string();
                let payload = LoginPayload { username: username.to_string(), password: password.to_string() };
                let record = self.seal(&id, &origin, &payload)?;
                let now = now_ms();
                let item = StoredItem { id: id.clone(), origin, created_at: now, updated_at: now, record };
                self.file.items.push(item);
                self.usernames.insert(id.clone(), username.to_string());
                if let Err(error) = self.persist() {
                    self.file.items.retain(|item| item.id != id);
                    self.usernames.remove(&id);
                    return Err(error);
                }
                Ok(self.summary(self.item(&id)?))
            }
        }
    }

    /// Changes a login's username and, when given, its password.
    pub fn update(&mut self, id: &str, username: &str, password: Option<&str>) -> Result<VaultItemSummary, VaultError> {
        let item = self.item(id)?.clone();
        let current = self.payload(&item)?;
        let password = password.unwrap_or(&current.password).to_string();
        validate(username, &password)?;
        let payload = LoginPayload { username: username.to_string(), password };
        let record = self.seal(id, &item.origin, &payload)?;
        let previous = (item.clone(), self.usernames.get(id).cloned());
        let index = self.file.items.iter().position(|stored| stored.id == id).ok_or_else(|| VaultError::NotFound(id.to_string()))?;
        self.file.items[index].record = record;
        self.file.items[index].updated_at = now_ms().max(item.updated_at + 1);
        self.usernames.insert(id.to_string(), username.to_string());
        if let Err(error) = self.persist() {
            self.file.items[index] = previous.0;
            match previous.1 {
                Some(name) => self.usernames.insert(id.to_string(), name),
                None => self.usernames.remove(id),
            };
            return Err(error);
        }
        Ok(self.summary(&self.file.items[index]))
    }

    pub fn delete(&mut self, id: &str) -> Result<(), VaultError> {
        let index = self.file.items.iter().position(|item| item.id == id).ok_or_else(|| VaultError::NotFound(id.to_string()))?;
        let removed = self.file.items.remove(index);
        if let Err(error) = self.persist() {
            self.file.items.insert(index, removed);
            return Err(error);
        }
        self.usernames.remove(id);
        Ok(())
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
