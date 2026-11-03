// allow: SIZE_OK — platform keychain security FFI and client pairing storage engine
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

use base64::prelude::*;
use ferryx_rd_proto::PAIRING_KEY_SIZE;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

const SWIFT_REFERENCE_DATE_OFFSET: f64 = 978_307_200.0;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingEndpoint {
    pub host: String,
    pub tcp_port: u16,
    pub udp_port: u16,
}

impl PairingEndpoint {
    pub fn new(host: impl Into<String>, tcp_port: u16, udp_port: u16) -> Self {
        Self {
            host: host.into(),
            tcp_port,
            udp_port,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRecord {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "deserialize_key", serialize_with = "serialize_key")]
    pub key: Vec<u8>,
    #[serde(
        rename = "addedAt",
        default,
        deserialize_with = "deserialize_added_at",
        serialize_with = "serialize_added_at",
        alias = "addedAt",
        alias = "added_at",
        alias = "added_at_unix_ms",
        alias = "addedAtUnixMs"
    )]
    pub added_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_endpoint: Option<PairingEndpoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoint_aliases: Vec<PairingEndpoint>,
    #[serde(default, rename = "relayUrl", skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    #[serde(
        default,
        rename = "relayHostId",
        skip_serializing_if = "Option::is_none"
    )]
    pub relay_host_id: Option<String>,
    #[serde(default, with = "trust_serde")]
    pub trust: ferryx_rd_proto::PairingTrust,
}

mod trust_serde {
    use super::*;
    use ferryx_rd_proto::WireCodec;

    pub fn serialize<S: Serializer>(
        trust: &ferryx_rd_proto::PairingTrust,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        trust
            .encode()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<ferryx_rd_proto::PairingTrust, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        ferryx_rd_proto::PairingTrust::decode(&bytes).map_err(de::Error::custom)
    }
}

impl fmt::Debug for PairingRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The pre-shared key must never appear in debug output or core dumps.
        f.debug_struct("PairingRecord")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("key", &"[REDACTED]")
            .field("added_at_unix_ms", &self.added_at_unix_ms)
            .field("last_endpoint", &self.last_endpoint)
            .field("endpoint_aliases", &self.endpoint_aliases)
            .field("relay_url", &self.relay_url)
            .field("relay_host_id", &self.relay_host_id)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSummary {
    pub id: String,
    pub host_name: String,
    pub added_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_endpoint: Option<PairingEndpoint>,
}

impl From<PairingRecord> for PairingSummary {
    fn from(record: PairingRecord) -> Self {
        Self {
            id: record.id,
            host_name: record.name,
            added_at_unix_ms: record.added_at_unix_ms,
            last_endpoint: record.last_endpoint,
        }
    }
}

impl From<&PairingRecord> for PairingSummary {
    fn from(record: &PairingRecord) -> Self {
        Self {
            id: record.id.clone(),
            host_name: record.name.clone(),
            added_at_unix_ms: record.added_at_unix_ms,
            last_endpoint: record.last_endpoint.clone(),
        }
    }
}

fn deserialize_key<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    struct KeyVisitor;

    impl<'de> de::Visitor<'de> for KeyVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a base64 string or byte array")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            BASE64_STANDARD
                .decode(value.trim())
                .map_err(de::Error::custom)
        }

        fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            self.visit_str(value)
        }

        fn visit_string<E>(self, value: String) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }

        fn visit_bytes<E>(self, value: &[u8]) -> Result<Vec<u8>, E>
        where
            E: de::Error,
        {
            Ok(value.to_vec())
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<u8>, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut bytes = Vec::new();
            while let Some(byte) = seq.next_element()? {
                bytes.push(byte);
            }
            Ok(bytes)
        }
    }

    deserializer.deserialize_any(KeyVisitor)
}

fn serialize_key<S>(key: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded = BASE64_STANDARD.encode(key);
    serializer.serialize_str(&encoded)
}

fn deserialize_added_at<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    struct AddedAtVisitor;

    impl<'de> de::Visitor<'de> for AddedAtVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a timestamp number")
        }

        fn visit_i64<E>(self, value: i64) -> Result<u64, E>
        where
            E: de::Error,
        {
            Ok(value.max(0) as u64)
        }

        fn visit_u64<E>(self, value: u64) -> Result<u64, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_f64<E>(self, value: f64) -> Result<u64, E>
        where
            E: de::Error,
        {
            if value > 1_000_000_000_000.0 {
                // Already unix ms
                Ok(value as u64)
            } else if value > 1_000_000_000.0 {
                // Unix seconds
                Ok((value * 1000.0) as u64)
            } else {
                // Swift reference date seconds (since 2001-01-01)
                let unix_secs = value + SWIFT_REFERENCE_DATE_OFFSET;
                Ok((unix_secs.max(0.0) * 1000.0) as u64)
            }
        }
    }

    deserializer.deserialize_any(AddedAtVisitor)
}

fn serialize_added_at<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let swift_time = (*value as f64 / 1000.0) - SWIFT_REFERENCE_DATE_OFFSET;
    serializer.serialize_f64(swift_time)
}

impl PairingRecord {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        key: Vec<u8>,
        added_at_unix_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            key,
            added_at_unix_ms,
            last_endpoint: None,
            endpoint_aliases: Vec::new(),
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        }
    }

    pub fn with_endpoints(
        mut self,
        last_endpoint: Option<PairingEndpoint>,
        endpoint_aliases: Vec<PairingEndpoint>,
    ) -> Self {
        self.last_endpoint = last_endpoint;
        self.endpoint_aliases = endpoint_aliases;
        self
    }

    pub fn summary(&self) -> PairingSummary {
        PairingSummary::from(self)
    }

    pub fn key_array(&self) -> Result<[u8; PAIRING_KEY_SIZE], PairingStoreError> {
        self.key
            .as_slice()
            .try_into()
            .map_err(|_| PairingStoreError::InvalidKeyLength(self.key.len()))
    }
}

#[derive(Debug, Clone)]
enum StoreBackend {
    File(PathBuf),
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    Keychain(String),
    Ephemeral(std::sync::Arc<std::sync::Mutex<Vec<PairingRecord>>>),
}

#[derive(Debug, Clone)]
pub struct PairingStore {
    backend: StoreBackend,
    fallback_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum PairingStoreError {
    #[error("pairing store I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("pairing store JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("pairing key has length {0}, expected 32")]
    InvalidKeyLength(usize),
    #[error("could not determine the user application data directory")]
    NoApplicationDataDirectory,
    #[error("keychain operation failed: {0}")]
    Keychain(String),
}

impl PairingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let p = path.into();
        Self {
            fallback_path: p.clone(),
            backend: StoreBackend::File(p),
        }
    }

    pub fn new_ephemeral() -> Self {
        Self {
            backend: StoreBackend::Ephemeral(std::sync::Arc::new(
                std::sync::Mutex::new(Vec::new()),
            )),
            fallback_path: PathBuf::new(),
        }
    }

    #[cfg(any(target_os = "ios", target_os = "macos"))]
    pub fn new_keychain(service: impl Into<String>) -> Self {
        Self {
            backend: StoreBackend::Keychain(service.into()),
            fallback_path: PathBuf::new(),
        }
    }

    pub fn default_path() -> Result<PathBuf, PairingStoreError> {
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or(PairingStoreError::NoApplicationDataDirectory)?;
            Ok(home
                .join("Library")
                .join("Application Support")
                .join("MahoRD")
                .join("client-pairings.json"))
        }
        #[cfg(target_os = "windows")]
        {
            let app_data = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or(PairingStoreError::NoApplicationDataDirectory)?;
            Ok(app_data.join("MahoRD").join("client-pairings.json"))
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
                })
                .ok_or(PairingStoreError::NoApplicationDataDirectory)?;
            Ok(base.join("MahoRD").join("client-pairings.json"))
        }
    }

    pub fn legacy_default_path() -> Result<PathBuf, PairingStoreError> {
        let default = Self::default_path()?;
        Ok(default.with_file_name("pairing-keys.json"))
    }

    pub fn open_default() -> Result<Self, PairingStoreError> {
        #[cfg(target_os = "ios")]
        {
            Ok(Self::new_keychain("app.ferryx.rd.pairing"))
        }
        #[cfg(not(target_os = "ios"))]
        {
            Ok(Self::new(Self::default_path()?))
        }
    }

    pub fn path(&self) -> &Path {
        match &self.backend {
            StoreBackend::File(p) => p.as_path(),
            _ => &self.fallback_path,
        }
    }

    pub fn load_all(&self) -> Result<Vec<PairingRecord>, PairingStoreError> {
        match &self.backend {
            StoreBackend::File(path) => {
                if !path.exists() {
                    // Safe legacy client migration: if client-pairings.json does not exist
                    // and a legacy store is present, migrate valid entries into client-pairings.json.
                    // The legacy files are preserved intact and never overwritten or deleted.
                    for legacy_path in Self::legacy_migration_candidates(path) {
                        if legacy_path.exists() && legacy_path != *path {
                            let legacy_records = Self::read_file_records(&legacy_path)?;
                            if !legacy_records.is_empty() {
                                self.write_records(path, &legacy_records)?;
                                return Ok(legacy_records);
                            }
                        }
                    }
                }
                Self::read_file_records(path)
            }
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => {
                let records = load_all_keychain(service)?;
                if !is_legacy_migration_completed(service) {
                    mark_legacy_migration_completed(service)?;
                    if records.is_empty() {
                        return migrate_legacy_keychain(service);
                    }
                }
                Ok(records)
            }
            StoreBackend::Ephemeral(records) => {
                let guard = records
                    .lock()
                    .map_err(|_| io::Error::other("ephemeral store poisoned"))?;
                Ok(guard.clone())
            }
        }
    }

    /// Legacy store locations, searched in order, when the current file is absent:
    /// the pre-rename file beside it, then both names inside the pre-rebrand
    /// `EclipticRD` application-data directory.
    fn legacy_migration_candidates(path: &Path) -> Vec<PathBuf> {
        let mut candidates = vec![path.with_file_name("pairing-keys.json")];
        if let Some(parent) = path.parent() {
            let legacy_dir = parent.with_file_name("EclipticRD");
            if legacy_dir != parent {
                if let Some(file_name) = path.file_name() {
                    candidates.push(legacy_dir.join(file_name));
                }
                candidates.push(legacy_dir.join("pairing-keys.json"));
            }
        }
        candidates
    }

    fn read_file_records(path: &Path) -> Result<Vec<PairingRecord>, PairingStoreError> {
        match fs::read(path) {
            Ok(bytes) => {
                let records: Vec<PairingRecord> = serde_json::from_slice(&bytes)?;
                // A single malformed record must not brick the whole store:
                // skip it and keep every valid pairing loadable.
                Ok(records
                    .into_iter()
                    .filter(|record| match record.key_array() {
                        Ok(_) => true,
                        Err(error) => {
                            tracing::warn!(
                                pairing_id = %record.id,
                                %error,
                                "skipping malformed pairing record"
                            );
                            false
                        }
                    })
                    .collect())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn load(&self, id: &str) -> Result<Option<PairingRecord>, PairingStoreError> {
        match &self.backend {
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => match load_keychain(service, id)? {
                Some(record) => Ok(Some(record)),
                // Nothing under the current service: load_all runs the legacy
                // migration, after which the record may exist locally.
                None => Ok(self.load_all()?.into_iter().find(|record| record.id == id)),
            },
            _ => Ok(self.load_all()?.into_iter().find(|record| record.id == id)),
        }
    }

    /// Latest record whose host name matches `host_name` — the Parsec-style
    /// "connect by computer name" lookup for reconnects without a PIN.
    pub fn find_by_host(
        &self,
        host_name: &str,
    ) -> Result<Option<PairingRecord>, PairingStoreError> {
        Ok(self
            .load_all()?
            .into_iter()
            .rfind(|record| record.name.eq_ignore_ascii_case(host_name) || record.id == host_name))
    }

    pub fn save(&self, record: PairingRecord) -> Result<(), PairingStoreError> {
        record.key_array()?;
        match &self.backend {
            StoreBackend::File(path) => {
                let _lock = StoreFileLock::acquire(path)?;
                let mut records = self.load_all()?;
                records.retain(|existing| existing.id != record.id);
                records.push(record);
                self.write_records(path, &records)
            }
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => save_keychain(service, &record),
            StoreBackend::Ephemeral(records) => {
                let mut guard = records
                    .lock()
                    .map_err(|_| io::Error::other("ephemeral store poisoned"))?;
                guard.retain(|existing| existing.id != record.id);
                guard.push(record);
                Ok(())
            }
        }
    }

    pub fn delete(&self, id: &str) -> Result<(), PairingStoreError> {
        match &self.backend {
            StoreBackend::File(path) => {
                let _lock = StoreFileLock::acquire(path)?;
                let mut records = self.load_all()?;
                records.retain(|record| record.id != id);
                self.write_records(path, &records)
            }
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => delete_keychain(service, id),
            StoreBackend::Ephemeral(records) => {
                let mut guard = records
                    .lock()
                    .map_err(|_| io::Error::other("ephemeral store poisoned"))?;
                guard.retain(|record| record.id != id);
                Ok(())
            }
        }
    }

    pub fn remember_endpoint(
        &self,
        id: &str,
        expected_key: &[u8],
        endpoint: PairingEndpoint,
    ) -> Result<bool, PairingStoreError> {
        match &self.backend {
            StoreBackend::File(path) => {
                let _lock = StoreFileLock::acquire(path)?;
                let mut records = self.load_all()?;
                let Some(record) = records.iter_mut().find(|r| r.id == id) else {
                    return Ok(false);
                };
                if record.key != expected_key {
                    return Ok(false);
                }
                update_record_endpoint(record, endpoint);
                self.write_records(path, &records)?;
                Ok(true)
            }
            #[cfg(any(target_os = "ios", target_os = "macos"))]
            StoreBackend::Keychain(service) => {
                let Some(mut record) = load_keychain(service, id)? else {
                    return Ok(false);
                };
                if record.key != expected_key {
                    return Ok(false);
                }
                update_record_endpoint(&mut record, endpoint);
                save_keychain(service, &record)?;
                Ok(true)
            }
            StoreBackend::Ephemeral(records) => {
                let mut guard = records
                    .lock()
                    .map_err(|_| io::Error::other("ephemeral store poisoned"))?;
                let Some(record) = guard.iter_mut().find(|r| r.id == id) else {
                    return Ok(false);
                };
                if record.key != expected_key {
                    return Ok(false);
                }
                update_record_endpoint(record, endpoint);
                Ok(true)
            }
        }
    }

    fn write_records(
        &self,
        path: &Path,
        records: &[PairingRecord],
    ) -> Result<(), PairingStoreError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "pairing store path has no parent",
            )
        })?;
        fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec(records)?;
        // A unique temporary in the target directory: concurrent writers (for
        // example the headless CLI beside the GUI client) cannot collide on a
        // fixed name, and create_new guarantees the private mode below applies
        // to a freshly created file rather than an inherited one.
        let stem = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pairings");
        let mut opened = None;
        for _ in 0..8 {
            let candidate = parent.join(format!(".{stem}.{}.tmp", rand::random::<u32>()));
            match open_private_new(&candidate) {
                Ok(file) => {
                    opened = Some((candidate, file));
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        let Some((temporary_path, file)) = opened else {
            return Err(PairingStoreError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not create a unique private temporary file",
            )));
        };
        let result = write_private_and_swap(&temporary_path, file, &bytes, path);
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary_path);
            return Err(error);
        }
        Ok(())
    }
}

fn open_private_new(path: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    }
}

fn write_private_and_swap(
    temporary_path: &Path,
    mut file: fs::File,
    bytes: &[u8],
    path: &Path,
) -> Result<(), PairingStoreError> {
    use std::io::Write;
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary_path, path)?;
        set_private_permissions(path)
    })();
    result.map_err(PairingStoreError::Io)
}

pub(crate) struct StoreFileLock {
    // Held only to keep the lock until drop.
    _file: fs::File,
}

#[cfg(unix)]
impl StoreFileLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, PairingStoreError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "pairing path has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("pairings");
        let lock_path = parent.join(format!(".{stem}.lock"));
        let file = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .mode(0o600)
                    .open(&lock_path)?
            }
            #[cfg(not(unix))]
            {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&lock_path)?
            }
        };
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)
            .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
impl Drop for StoreFileLock {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(&self._file, rustix::fs::FlockOperation::Unlock);
    }
}

#[cfg(not(unix))]
impl StoreFileLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, PairingStoreError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "pairing path has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("pairings");
        let lock_path = parent.join(format!(".{stem}.lock"));
        let start = std::time::Instant::now();
        loop {
            match fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
            {
                Ok(file) => return Ok(Self { _file: file }),
                Err(_) if start.elapsed() < std::time::Duration::from_secs(5) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// Most recent roaming endpoints retained per record; older aliases are dropped.
const MAX_ENDPOINT_ALIASES: usize = 16;

fn update_record_endpoint(record: &mut PairingRecord, endpoint: PairingEndpoint) {
    record.endpoint_aliases.retain(|a| a != &endpoint);
    if let Some(prev) = record.last_endpoint.take() {
        if prev != endpoint && !record.endpoint_aliases.contains(&prev) {
            record.endpoint_aliases.push(prev);
        }
    }
    // Bound roaming history: keep the most recent aliases, drop the oldest.
    while record.endpoint_aliases.len() > MAX_ENDPOINT_ALIASES {
        record.endpoint_aliases.remove(0);
    }
    record.last_endpoint = Some(endpoint);
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[allow(dead_code)]
mod security_ffi {
    use std::os::raw::c_void;

    pub type CFTypeRef = *const c_void;
    pub type CFStringRef = *const c_void;
    pub type CFDataRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CFArrayRef = *const c_void;
    pub type CFIndex = isize;
    pub type OSStatus = i32;

    pub const ERR_SEC_SUCCESS: OSStatus = 0;
    pub const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
    pub const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25299;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        pub static kSecClass: CFStringRef;
        pub static kSecClassGenericPassword: CFTypeRef;
        pub static kSecAttrService: CFStringRef;
        pub static kSecAttrAccount: CFStringRef;
        pub static kSecValueData: CFStringRef;
        pub static kSecReturnData: CFStringRef;
        pub static kSecReturnAttributes: CFStringRef;
        pub static kSecMatchLimit: CFStringRef;
        pub static kSecMatchLimitOne: CFTypeRef;
        pub static kSecMatchLimitAll: CFTypeRef;
        pub static kSecAttrAccessible: CFStringRef;
        pub static kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: CFTypeRef;

        pub fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        pub fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        pub fn SecItemUpdate(
            query: CFDictionaryRef,
            attributesToUpdate: CFDictionaryRef,
        ) -> OSStatus;
        pub fn SecItemDelete(query: CFDictionaryRef) -> OSStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFBooleanTrue: CFTypeRef;
        pub static kCFTypeDictionaryKeyCallBacks: c_void;
        pub static kCFTypeDictionaryValueCallBacks: c_void;

        pub fn CFStringCreateWithBytes(
            alloc: CFTypeRef,
            bytes: *const u8,
            numBytes: CFIndex,
            encoding: u32,
            isExternalRepresentation: u8,
        ) -> CFStringRef;
        pub fn CFDataCreate(alloc: CFTypeRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
        pub fn CFDataGetLength(theData: CFDataRef) -> CFIndex;
        pub fn CFDataGetBytePtr(theData: CFDataRef) -> *const u8;
        pub fn CFDictionaryCreate(
            alloc: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            numValues: CFIndex,
            keyCallBacks: *const c_void,
            valueCallBacks: *const c_void,
        ) -> CFDictionaryRef;
        pub fn CFDictionaryGetValue(theDict: CFDictionaryRef, theKey: CFTypeRef) -> CFTypeRef;
        pub fn CFArrayGetCount(theArray: CFArrayRef) -> CFIndex;
        pub fn CFArrayGetValueAtIndex(theArray: CFArrayRef, idx: CFIndex) -> CFTypeRef;
        pub fn CFStringGetCString(
            theString: CFStringRef,
            buffer: *mut u8,
            bufferSize: CFIndex,
            encoding: u32,
        ) -> u8;
        pub fn CFStringGetLength(theString: CFStringRef) -> CFIndex;
        pub fn CFRelease(cf: CFTypeRef);
    }
    pub const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
struct CfWrapper<T>(*const T);

#[cfg(any(target_os = "ios", target_os = "macos"))]
impl<T> Drop for CfWrapper<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                security_ffi::CFRelease(self.0 as security_ffi::CFTypeRef);
            }
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_string(s: &str) -> Option<CfWrapper<std::os::raw::c_void>> {
    unsafe {
        let cf = security_ffi::CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as isize,
            security_ffi::K_CF_STRING_ENCODING_UTF8,
            0,
        );
        if cf.is_null() {
            None
        } else {
            Some(CfWrapper(cf))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_data(bytes: &[u8]) -> Option<CfWrapper<std::os::raw::c_void>> {
    unsafe {
        let cf = security_ffi::CFDataCreate(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize);
        if cf.is_null() {
            None
        } else {
            Some(CfWrapper(cf))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn make_cf_dictionary(
    pairs: &[(security_ffi::CFTypeRef, security_ffi::CFTypeRef)],
) -> Option<CfWrapper<std::os::raw::c_void>> {
    let mut keys = Vec::with_capacity(pairs.len());
    let mut values = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        keys.push(*k);
        values.push(*v);
    }
    unsafe {
        let dict = security_ffi::CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            pairs.len() as isize,
            &security_ffi::kCFTypeDictionaryKeyCallBacks as *const _ as *const _,
            &security_ffi::kCFTypeDictionaryValueCallBacks as *const _ as *const _,
        );
        if dict.is_null() {
            None
        } else {
            Some(CfWrapper(dict))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn cf_data_to_vec(data: security_ffi::CFDataRef) -> Vec<u8> {
    unsafe {
        let len = security_ffi::CFDataGetLength(data) as usize;
        let ptr = security_ffi::CFDataGetBytePtr(data);
        if ptr.is_null() || len == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(ptr, len).to_vec()
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn save_keychain(service: &str, record: &PairingRecord) -> Result<(), PairingStoreError> {
    let key = format!("maho_pairing_{}", record.id);
    let bytes = serde_json::to_vec(record)?;
    unsafe {
        let service_cf = make_cf_string(service).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate service CFString".into())
        })?;
        let account_cf = make_cf_string(&key).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate account CFString".into())
        })?;
        let data_cf = make_cf_data(&bytes)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate data CFData".into()))?;

        let pairs = [
            (
                security_ffi::kSecClass as security_ffi::CFTypeRef,
                security_ffi::kSecClassGenericPassword,
            ),
            (
                security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                service_cf.0,
            ),
            (
                security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                account_cf.0,
            ),
            (
                security_ffi::kSecValueData as security_ffi::CFTypeRef,
                data_cf.0,
            ),
            (
                security_ffi::kSecAttrAccessible as security_ffi::CFTypeRef,
                security_ffi::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
            ),
        ];
        let dict = make_cf_dictionary(&pairs)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate CFDictionary".into()))?;

        let status = security_ffi::SecItemAdd(dict.0, std::ptr::null_mut());
        if status == security_ffi::ERR_SEC_DUPLICATE_ITEM {
            let query_pairs = [
                (
                    security_ffi::kSecClass as security_ffi::CFTypeRef,
                    security_ffi::kSecClassGenericPassword,
                ),
                (
                    security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                    service_cf.0,
                ),
                (
                    security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                    account_cf.0,
                ),
            ];
            let query_dict = make_cf_dictionary(&query_pairs).ok_or_else(|| {
                PairingStoreError::Keychain("Failed to allocate query CFDictionary".into())
            })?;
            let update_pairs = [(
                security_ffi::kSecValueData as security_ffi::CFTypeRef,
                data_cf.0,
            )];
            let update_dict = make_cf_dictionary(&update_pairs).ok_or_else(|| {
                PairingStoreError::Keychain("Failed to allocate update CFDictionary".into())
            })?;

            let update_status = security_ffi::SecItemUpdate(query_dict.0, update_dict.0);
            if update_status != security_ffi::ERR_SEC_SUCCESS {
                return Err(PairingStoreError::Keychain(format!(
                    "SecItemUpdate failed: OSStatus {update_status}"
                )));
            }
            Ok(())
        } else if status != security_ffi::ERR_SEC_SUCCESS {
            Err(PairingStoreError::Keychain(format!(
                "SecItemAdd failed: OSStatus {status}"
            )))
        } else {
            Ok(())
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn load_keychain(service: &str, id: &str) -> Result<Option<PairingRecord>, PairingStoreError> {
    let key = format!("maho_pairing_{id}");
    unsafe {
        let service_cf = make_cf_string(service).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate service CFString".into())
        })?;
        let account_cf = make_cf_string(&key).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate account CFString".into())
        })?;

        let query_pairs = [
            (
                security_ffi::kSecClass as security_ffi::CFTypeRef,
                security_ffi::kSecClassGenericPassword,
            ),
            (
                security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                service_cf.0,
            ),
            (
                security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                account_cf.0,
            ),
            (
                security_ffi::kSecReturnData as security_ffi::CFTypeRef,
                security_ffi::kCFBooleanTrue,
            ),
            (
                security_ffi::kSecMatchLimit as security_ffi::CFTypeRef,
                security_ffi::kSecMatchLimitOne,
            ),
        ];
        let query = make_cf_dictionary(&query_pairs).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate query CFDictionary".into())
        })?;

        let mut result: security_ffi::CFTypeRef = std::ptr::null();
        let status = security_ffi::SecItemCopyMatching(query.0, &mut result);
        if status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
            return Ok(None);
        }
        if status != security_ffi::ERR_SEC_SUCCESS {
            return Err(PairingStoreError::Keychain(format!(
                "SecItemCopyMatching failed: OSStatus {status}"
            )));
        }
        if result.is_null() {
            return Ok(None);
        }
        let wrapper = CfWrapper(result);
        let bytes = cf_data_to_vec(wrapper.0);
        let record: PairingRecord = serde_json::from_slice(&bytes)?;
        record.key_array()?;
        Ok(Some(record))
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn load_all_keychain(service: &str) -> Result<Vec<PairingRecord>, PairingStoreError> {
    load_keychain_matching(service, None)
}

/// Pre-rebrand Keychain service and account prefixes, migrated on first load.
#[cfg(any(target_os = "ios", target_os = "macos"))]
const LEGACY_KEYCHAIN_SERVICE: &str = "com.eclipticrd.ios.pairing";
#[cfg(any(target_os = "ios", target_os = "macos"))]
const LEGACY_KEYCHAIN_ACCOUNT_PREFIXES: [&str; 2] = ["erd_pairing_", "maho_pairing_"];

/// Imports pairings from the legacy EclipticRD Keychain service into `service`.
/// The legacy items are only read here and are left in place.
#[cfg(any(target_os = "ios", target_os = "macos"))]
#[cfg(any(target_os = "ios", target_os = "macos"))]
const MIGRATION_MARKER_ACCOUNT: &str = "maho_migration_completed_v1";

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn is_legacy_migration_completed(service: &str) -> bool {
    has_keychain_account(service, MIGRATION_MARKER_ACCOUNT).unwrap_or(false)
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn mark_legacy_migration_completed(service: &str) -> Result<(), PairingStoreError> {
    save_raw_keychain(service, MIGRATION_MARKER_ACCOUNT, b"completed")
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn has_keychain_account(service: &str, account: &str) -> Result<bool, PairingStoreError> {
    unsafe {
        let service_cf = make_cf_string(service).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate service CFString".into())
        })?;
        let account_cf = make_cf_string(account).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate account CFString".into())
        })?;
        let query_pairs = [
            (
                security_ffi::kSecClass as security_ffi::CFTypeRef,
                security_ffi::kSecClassGenericPassword,
            ),
            (
                security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                service_cf.0,
            ),
            (
                security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                account_cf.0,
            ),
        ];
        let query = make_cf_dictionary(&query_pairs).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate query CFDictionary".into())
        })?;
        let status = security_ffi::SecItemCopyMatching(query.0, std::ptr::null_mut());
        if status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
            Ok(false)
        } else if status == security_ffi::ERR_SEC_SUCCESS {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn save_raw_keychain(service: &str, account: &str, data: &[u8]) -> Result<(), PairingStoreError> {
    unsafe {
        let service_cf = make_cf_string(service).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate service CFString".into())
        })?;
        let account_cf = make_cf_string(account).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate account CFString".into())
        })?;
        let data_cf = make_cf_data(data)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate data CFData".into()))?;

        let pairs = [
            (
                security_ffi::kSecClass as security_ffi::CFTypeRef,
                security_ffi::kSecClassGenericPassword,
            ),
            (
                security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                service_cf.0,
            ),
            (
                security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                account_cf.0,
            ),
            (
                security_ffi::kSecValueData as security_ffi::CFTypeRef,
                data_cf.0,
            ),
            (
                security_ffi::kSecAttrAccessible as security_ffi::CFTypeRef,
                security_ffi::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
            ),
        ];
        let dict = make_cf_dictionary(&pairs)
            .ok_or_else(|| PairingStoreError::Keychain("Failed to allocate CFDictionary".into()))?;

        let status = security_ffi::SecItemAdd(dict.0, std::ptr::null_mut());
        if status == security_ffi::ERR_SEC_DUPLICATE_ITEM || status == security_ffi::ERR_SEC_SUCCESS
        {
            Ok(())
        } else {
            Err(PairingStoreError::Keychain(format!(
                "SecItemAdd failed: OSStatus {status}"
            )))
        }
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn migrate_legacy_keychain(service: &str) -> Result<Vec<PairingRecord>, PairingStoreError> {
    if service == LEGACY_KEYCHAIN_SERVICE {
        return Ok(Vec::new());
    }
    let legacy = load_keychain_matching(
        LEGACY_KEYCHAIN_SERVICE,
        Some(&LEGACY_KEYCHAIN_ACCOUNT_PREFIXES),
    )?;
    for record in &legacy {
        save_keychain(service, record)?;
    }
    Ok(legacy)
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn cf_string_to_string(value: security_ffi::CFStringRef) -> Option<String> {
    unsafe {
        let length = security_ffi::CFStringGetLength(value);
        if length <= 0 {
            return None;
        }
        let mut buffer = vec![0_u8; length as usize * 4 + 1];
        let copied = security_ffi::CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as security_ffi::CFIndex,
            security_ffi::K_CF_STRING_ENCODING_UTF8,
        );
        if copied == 0 {
            return None;
        }
        let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(0);
        String::from_utf8(buffer[..end].to_vec()).ok()
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn load_keychain_matching(
    service: &str,
    account_prefixes: Option<&[&str]>,
) -> Result<Vec<PairingRecord>, PairingStoreError> {
    unsafe {
        let service_cf = make_cf_string(service).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate service CFString".into())
        })?;

        let query_pairs = [
            (
                security_ffi::kSecClass as security_ffi::CFTypeRef,
                security_ffi::kSecClassGenericPassword,
            ),
            (
                security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                service_cf.0,
            ),
            (
                security_ffi::kSecReturnAttributes as security_ffi::CFTypeRef,
                security_ffi::kCFBooleanTrue,
            ),
            (
                security_ffi::kSecReturnData as security_ffi::CFTypeRef,
                security_ffi::kCFBooleanTrue,
            ),
            (
                security_ffi::kSecMatchLimit as security_ffi::CFTypeRef,
                security_ffi::kSecMatchLimitAll,
            ),
        ];
        let query = make_cf_dictionary(&query_pairs).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate query CFDictionary".into())
        })?;

        let mut result: security_ffi::CFTypeRef = std::ptr::null();
        let status = security_ffi::SecItemCopyMatching(query.0, &mut result);
        if status == security_ffi::ERR_SEC_ITEM_NOT_FOUND {
            return Ok(Vec::new());
        }
        if status != security_ffi::ERR_SEC_SUCCESS {
            return Err(PairingStoreError::Keychain(format!(
                "SecItemCopyMatching list failed: OSStatus {status}"
            )));
        }
        if result.is_null() {
            return Ok(Vec::new());
        }
        let wrapper = CfWrapper(result);
        let count = security_ffi::CFArrayGetCount(wrapper.0);
        let mut records = Vec::new();
        for i in 0..count {
            let dict = security_ffi::CFArrayGetValueAtIndex(wrapper.0, i);
            if !dict.is_null() {
                if let Some(prefixes) = account_prefixes {
                    let account_val = security_ffi::CFDictionaryGetValue(
                        dict,
                        security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                    );
                    let account = if account_val.is_null() {
                        None
                    } else {
                        cf_string_to_string(account_val)
                    };
                    let matches = account.is_some_and(|account| {
                        prefixes.iter().any(|prefix| account.starts_with(prefix))
                    });
                    if !matches {
                        continue;
                    }
                }
                let data_val = security_ffi::CFDictionaryGetValue(
                    dict,
                    security_ffi::kSecValueData as security_ffi::CFTypeRef,
                );
                if !data_val.is_null() {
                    let bytes = cf_data_to_vec(data_val);
                    if let Ok(record) = serde_json::from_slice::<PairingRecord>(&bytes) {
                        if record.key_array().is_ok() {
                            records.push(record);
                        }
                    }
                }
            }
        }
        Ok(records)
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn delete_keychain(service: &str, id: &str) -> Result<(), PairingStoreError> {
    let key = format!("maho_pairing_{id}");
    unsafe {
        let service_cf = make_cf_string(service).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate service CFString".into())
        })?;
        let account_cf = make_cf_string(&key).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate account CFString".into())
        })?;

        let query_pairs = [
            (
                security_ffi::kSecClass as security_ffi::CFTypeRef,
                security_ffi::kSecClassGenericPassword,
            ),
            (
                security_ffi::kSecAttrService as security_ffi::CFTypeRef,
                service_cf.0,
            ),
            (
                security_ffi::kSecAttrAccount as security_ffi::CFTypeRef,
                account_cf.0,
            ),
        ];
        let query = make_cf_dictionary(&query_pairs).ok_or_else(|| {
            PairingStoreError::Keychain("Failed to allocate query CFDictionary".into())
        })?;

        let status = security_ffi::SecItemDelete(query.0);
        if status == security_ffi::ERR_SEC_SUCCESS || status == security_ffi::ERR_SEC_ITEM_NOT_FOUND
        {
            Ok(())
        } else {
            Err(PairingStoreError::Keychain(format!(
                "SecItemDelete failed: OSStatus {status}"
            )))
        }
    }
}

fn set_private_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_default_path_filename() {
        let path = PairingStore::default_path().unwrap();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("client-pairings.json"),
            "Client default path must be client-pairings.json"
        );
        let legacy_path = PairingStore::legacy_default_path().unwrap();
        assert_eq!(
            legacy_path.file_name().and_then(|n| n.to_str()),
            Some("pairing-keys.json"),
            "Client legacy path must be pairing-keys.json"
        );
        assert_eq!(
            path.parent(),
            legacy_path.parent(),
            "Client default and legacy files reside in the same MahoRD directory"
        );
    }

    #[test]
    fn test_legacy_client_migration_preserves_legacy_and_writes_client_pairings() {
        let temp_dir = tempfile::tempdir().unwrap();
        let legacy_file = temp_dir.path().join("pairing-keys.json");
        let client_file = temp_dir.path().join("client-pairings.json");

        let legacy_json = serde_json::json!([
            {
                "id": "legacy-id-42",
                "name": "LegacyDesktop",
                "key": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
                "addedAt": 0.0
            }
        ]);
        let legacy_bytes = serde_json::to_vec_pretty(&legacy_json).unwrap();
        fs::write(&legacy_file, &legacy_bytes).unwrap();

        assert!(!client_file.exists());
        let store = PairingStore::new(&client_file);
        let records = store.load_all().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "legacy-id-42");
        assert_eq!(records[0].name, "LegacyDesktop");
        assert_eq!(records[0].key, vec![1u8; 32]);

        // client-pairings.json was created
        assert!(client_file.exists());

        // legacy pairing-keys.json is preserved completely intact
        let current_legacy_bytes = fs::read(&legacy_file).unwrap();
        assert_eq!(
            current_legacy_bytes, legacy_bytes,
            "Legacy file must be preserved byte-for-byte during migration"
        );
    }

    #[test]
    fn test_legacy_client_migration_skips_when_client_pairings_already_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let legacy_file = temp_dir.path().join("pairing-keys.json");
        let client_file = temp_dir.path().join("client-pairings.json");

        let legacy_json = serde_json::json!([
            {
                "id": "legacy-id-old",
                "name": "OldHost",
                "key": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
                "addedAt": 0.0
            }
        ]);
        fs::write(&legacy_file, serde_json::to_vec(&legacy_json).unwrap()).unwrap();

        let client_json = serde_json::json!([
            {
                "id": "client-id-new",
                "name": "NewHost",
                "key": "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=",
                "addedAt": 100.0
            }
        ]);
        fs::write(&client_file, serde_json::to_vec(&client_json).unwrap()).unwrap();

        let store = PairingStore::new(&client_file);
        let records = store.load_all().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "client-id-new");
        assert_eq!(records[0].name, "NewHost");
    }

    #[test]
    fn test_deletion_isolated_from_legacy() {
        let temp_dir = tempfile::tempdir().unwrap();
        let legacy_file = temp_dir.path().join("pairing-keys.json");
        let client_file = temp_dir.path().join("client-pairings.json");

        let legacy_json = serde_json::json!([
            {
                "id": "legacy-to-delete",
                "name": "DeleteMe",
                "key": "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM=",
                "addedAt": 0.0
            }
        ]);
        let legacy_bytes = serde_json::to_vec_pretty(&legacy_json).unwrap();
        fs::write(&legacy_file, &legacy_bytes).unwrap();

        let store = PairingStore::new(&client_file);
        assert_eq!(store.load_all().unwrap().len(), 1);

        // Delete from client store
        store.delete("legacy-to-delete").unwrap();
        assert!(store.load_all().unwrap().is_empty());
        assert_eq!(store.load("legacy-to-delete").unwrap(), None);

        // Legacy file must remain untouched
        let remaining_legacy = fs::read(&legacy_file).unwrap();
        assert_eq!(remaining_legacy, legacy_bytes);
    }

    #[test]
    fn test_legacy_eclipticrd_directory_migration() {
        let temp_dir = tempfile::tempdir().unwrap();
        let legacy_dir = temp_dir.path().join("EclipticRD");
        let current_dir = temp_dir.path().join("MahoRD");
        fs::create_dir_all(&legacy_dir).unwrap();
        let legacy_file = legacy_dir.join("pairing-keys.json");
        let client_file = current_dir.join("client-pairings.json");

        let legacy_json = serde_json::json!([
            {
                "id": "erd-id-7",
                "name": "EclipticHost",
                "key": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
                "addedAt": 0.0
            }
        ]);
        let legacy_bytes = serde_json::to_vec(&legacy_json).unwrap();
        fs::write(&legacy_file, &legacy_bytes).unwrap();

        let store = PairingStore::new(&client_file);
        let records = store.load_all().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "erd-id-7");
        assert!(client_file.exists());
        assert_eq!(
            fs::read(&legacy_file).unwrap(),
            legacy_bytes,
            "legacy EclipticRD file must be preserved"
        );
    }

    #[test]
    fn test_malformed_record_is_skipped_not_fatal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let client_file = temp_dir.path().join("client-pairings.json");
        let json = serde_json::json!([
            {
                "id": "broken",
                "name": "BrokenHost",
                "key": "AQEB",
                "addedAt": 0.0
            },
            {
                "id": "intact",
                "name": "IntactHost",
                "key": "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=",
                "addedAt": 0.0
            }
        ]);
        fs::write(&client_file, serde_json::to_vec(&json).unwrap()).unwrap();

        let store = PairingStore::new(&client_file);
        let records = store.load_all().unwrap();
        assert_eq!(records.len(), 1, "only the valid record must load");
        assert_eq!(records[0].id, "intact");
        assert_eq!(
            store.load("intact").unwrap().map(|r| r.id),
            Some("intact".to_owned())
        );
        assert_eq!(store.load("broken").unwrap(), None);
    }

    #[test]
    fn test_temporary_file_naming_isolated() {
        let temp_dir = tempfile::tempdir().unwrap();
        let client_file = temp_dir.path().join("client-pairings.json");
        let store = PairingStore::new(&client_file);

        let record = PairingRecord::new("temp-test-1", "TempHost", vec![0x11; 32], 1700000000000);
        store.save(record).unwrap();

        // Ensure temporary file was client-pairings.json.tmp and was cleanly renamed
        let tmp_file = client_file.with_extension("json.tmp");
        assert!(
            !tmp_file.exists(),
            "Temporary file must be cleaned up / renamed"
        );
        assert!(client_file.exists());
    }

    #[test]
    fn ephemeral_pairing_store_roundtrip_and_delete() {
        let store = PairingStore::new_ephemeral();
        let record =
            PairingRecord::new("host-ephemeral-1", "Server", vec![0x55; 32], 1700000000000);

        store.save(record.clone()).unwrap();
        assert_eq!(
            store.load("host-ephemeral-1").unwrap(),
            Some(record.clone())
        );
        assert_eq!(store.find_by_host("server").unwrap(), Some(record.clone()));
        assert_eq!(
            store.find_by_host("host-ephemeral-1").unwrap(),
            Some(record.clone())
        );

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "host-ephemeral-1");

        store.delete("host-ephemeral-1").unwrap();
        assert_eq!(store.load("host-ephemeral-1").unwrap(), None);
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn test_legacy_record_readability() {
        // Given: legacy JSON without lastEndpoint or endpointAliases
        let legacy_json = r#"[
          {
            "id": "legacy-id-100",
            "name": "LegacyServer",
            "key": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
            "addedAt": 0.0
          }
        ]"#;

        // When: deserialized into Vec<PairingRecord>
        let records: Vec<PairingRecord> = serde_json::from_str(legacy_json).unwrap();

        // Then: legacy fields parse correctly, optional endpoint fields default to None / empty
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.id, "legacy-id-100");
        assert_eq!(r.name, "LegacyServer");
        assert_eq!(r.key, vec![1u8; 32]);
        assert_eq!(r.added_at_unix_ms, 978_307_200_000);
        assert_eq!(r.last_endpoint, None);
        assert!(r.endpoint_aliases.is_empty());
        assert!(r.relay_url.is_none());
        assert!(r.relay_host_id.is_none());
    }

    #[test]
    fn pairing_round_trip_keeps_relay_url() {
        let mut record = PairingRecord::new("id-1", "host", vec![2_u8; 32], 1);
        record.relay_url = Some("wss://relay.example".to_owned());
        record.relay_host_id = Some("host-1".to_owned());
        let json = serde_json::to_string(&record).expect("serialize");
        assert!(
            json.contains("\"relayUrl\":\"wss://relay.example\"")
                || json.contains("\"relayUrl\": \"wss://relay.example\"")
        );
        let parsed: PairingRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.relay_url.as_deref(), Some("wss://relay.example"));
        assert_eq!(parsed.relay_host_id.as_deref(), Some("host-1"));
    }

    #[test]
    fn test_metadata_roundtrip_and_dedup() {
        // Given: an ephemeral store seeded with a pairing record
        let store = PairingStore::new_ephemeral();
        let key = vec![0x22; 32];
        let record = PairingRecord::new("host-id-1", "HostOne", key.clone(), 1700000000000);
        store.save(record).unwrap();

        let ep1 = PairingEndpoint::new("192.168.1.50", 19730, 19731);
        let ep2 = PairingEndpoint::new("100.91.254.71", 19730, 19731);

        // When: remember_endpoint called for ep1
        let updated = store
            .remember_endpoint("host-id-1", &key, ep1.clone())
            .unwrap();
        assert!(updated);

        // Then: last_endpoint is ep1, aliases empty
        let loaded = store.load("host-id-1").unwrap().unwrap();
        assert_eq!(loaded.last_endpoint, Some(ep1.clone()));
        assert!(loaded.endpoint_aliases.is_empty());

        // When: remember_endpoint called again with same ep1 (idempotent / dedup)
        let updated2 = store
            .remember_endpoint("host-id-1", &key, ep1.clone())
            .unwrap();
        assert!(updated2);

        // Then: last_endpoint remains ep1, aliases still empty (no duplicate added)
        let loaded2 = store.load("host-id-1").unwrap().unwrap();
        assert_eq!(loaded2.last_endpoint, Some(ep1.clone()));
        assert!(loaded2.endpoint_aliases.is_empty());

        // When: remember_endpoint called with ep2
        let updated3 = store
            .remember_endpoint("host-id-1", &key, ep2.clone())
            .unwrap();
        assert!(updated3);

        // Then: last_endpoint is ep2, ep1 is shifted to aliases
        let loaded3 = store.load("host-id-1").unwrap().unwrap();
        assert_eq!(loaded3.last_endpoint, Some(ep2.clone()));
        assert_eq!(loaded3.endpoint_aliases, vec![ep1.clone()]);

        // When: remember_endpoint called with ep1 again
        let updated4 = store
            .remember_endpoint("host-id-1", &key, ep1.clone())
            .unwrap();
        assert!(updated4);

        // Then: last_endpoint is ep1, ep2 is in aliases, ep1 is deduped from aliases
        let loaded4 = store.load("host-id-1").unwrap().unwrap();
        assert_eq!(loaded4.last_endpoint, Some(ep1.clone()));
        assert_eq!(loaded4.endpoint_aliases, vec![ep2.clone()]);

        // Serialization roundtrip retains metadata
        let json = serde_json::to_string(&loaded4).unwrap();
        let deserialized: PairingRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, loaded4);
    }

    #[test]
    fn test_preservation_of_credentials() {
        // Given: store with a record having specific key and metadata
        let store = PairingStore::new_ephemeral();
        let original_key = vec![0x77; 32];
        let original_id = "cred-preserve-id";
        let original_name = "PreservedHost";
        let original_time = 1712345678900;
        let record = PairingRecord::new(
            original_id,
            original_name,
            original_key.clone(),
            original_time,
        );
        store.save(record).unwrap();

        let ep = PairingEndpoint::new("10.0.0.5", 19730, 19731);

        // When: remember_endpoint updates endpoint metadata
        let updated = store
            .remember_endpoint(original_id, &original_key, ep.clone())
            .unwrap();
        assert!(updated);

        // Then: secret key bytes, id, name, and added_at_unix_ms remain strictly identical
        let loaded = store.load(original_id).unwrap().unwrap();
        assert_eq!(loaded.id, original_id);
        assert_eq!(loaded.name, original_name);
        assert_eq!(loaded.key, original_key);
        assert_eq!(loaded.added_at_unix_ms, original_time);
        assert_eq!(loaded.last_endpoint, Some(ep));
    }

    #[test]
    fn test_missing_record_no_resurrection() {
        // Given: an empty store (record never existed or was deleted)
        let store = PairingStore::new_ephemeral();
        let key = vec![0x33; 32];
        let ep = PairingEndpoint::new("192.168.1.1", 19730, 19731);

        // When: remember_endpoint called for non-existent ID
        let updated = store
            .remember_endpoint("non-existent-uuid", &key, ep)
            .unwrap();

        // Then: returns false and does NOT resurrect or create any record
        assert!(!updated);
        assert!(store.load("non-existent-uuid").unwrap().is_none());
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn test_id_key_mismatch_no_mutation() {
        // Given: store with an existing record
        let store = PairingStore::new_ephemeral();
        let correct_key = vec![0x44; 32];
        let wrong_key = vec![0x99; 32];
        let record = PairingRecord::new(
            "target-id",
            "TargetHost",
            correct_key.clone(),
            1700000000000,
        );
        store.save(record.clone()).unwrap();

        let ep = PairingEndpoint::new("192.168.1.200", 19730, 19731);

        // When: remember_endpoint called with mismatched expected_key
        let updated = store
            .remember_endpoint("target-id", &wrong_key, ep)
            .unwrap();

        // Then: returns false and stored record is completely unmutated
        assert!(!updated);
        let loaded = store.load("target-id").unwrap().unwrap();
        assert_eq!(loaded, record);
        assert_eq!(loaded.last_endpoint, None);
    }

    #[test]
    fn test_key_free_summary_serialization() {
        // Given: a PairingRecord with endpoint metadata and secret key
        let key = vec![0xDE; 32];
        let record = PairingRecord {
            id: "summary-test-id".into(),
            name: "SummaryHost".into(),
            key: key.clone(),
            added_at_unix_ms: 1720000000000,
            last_endpoint: Some(PairingEndpoint::new("[fe80::1%en0]", 19730, 19731)),
            endpoint_aliases: vec![],
            relay_url: None,
            relay_host_id: None,
            trust: ferryx_rd_proto::PairingTrust::Owner,
        };

        // When: converted to PairingSummary and serialized to JSON
        let summary = record.summary();
        let json = serde_json::to_string(&summary).unwrap();

        // Then: serialized JSON contains ONLY allowed machine fields, and ZERO secret keys
        assert!(!json.contains("key"));
        assert!(!json.contains("DEADBEEF"));
        assert!(!json.contains("3q2+7w==")); // base64 of DEAD...

        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = val.as_object().unwrap();
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec!["addedAtUnixMs", "hostName", "id", "lastEndpoint"]
        );

        // Endpoint preserves IPv6 scope and concrete ports
        let ep_val = &val["lastEndpoint"];
        assert_eq!(ep_val["host"], "[fe80::1%en0]");
        assert_eq!(ep_val["tcpPort"], 19730);
        assert_eq!(ep_val["udpPort"], 19731);
    }

    #[test]
    fn pairing_record_debug_redacts_the_secret_key() {
        let secret: &[u8] = &[0xAB; 32];
        let record = PairingRecord::new("debug-id", "DebugHost", secret.to_vec(), 0);
        let rendered = format!("{record:?}");
        assert!(rendered.contains("[REDACTED]"), "got {rendered}");
        assert!(!rendered.contains(&format!("{:?}", secret)));
        assert!(
            !rendered.contains("171"),
            "decimal key bytes leaked: {rendered}"
        );
    }

    #[test]
    fn endpoint_aliases_stay_bounded_while_roaming() {
        let store = PairingStore::new_ephemeral();
        let key = vec![0x77; 32];
        store
            .save(PairingRecord::new("roam-id", "RoamHost", key.clone(), 0))
            .unwrap();
        let mut latest = None;
        for i in 0..40_u8 {
            let endpoint = PairingEndpoint::new(format!("10.0.0.{i}"), 19730, 19731);
            store
                .remember_endpoint("roam-id", &key, endpoint.clone())
                .unwrap();
            latest = Some(endpoint);
        }
        let loaded = store.load("roam-id").unwrap().unwrap();
        assert_eq!(loaded.last_endpoint, latest);
        assert!(
            loaded.endpoint_aliases.len() <= 16,
            "alias history is unbounded: {}",
            loaded.endpoint_aliases.len()
        );
        // The most recent aliases survive truncation: 10.0.0.39 is the current
        // last_endpoint (never an alias), so 10.0.0.38 is the newest alias.
        assert!(loaded
            .endpoint_aliases
            .contains(&PairingEndpoint::new("10.0.0.38", 19730, 19731)));
        assert!(!loaded
            .endpoint_aliases
            .contains(&PairingEndpoint::new("10.0.0.0", 19730, 19731)));
    }

    #[cfg(unix)]
    #[test]
    fn saved_pairings_use_a_unique_private_temporary_and_leave_no_strays() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = tempfile::tempdir().unwrap();
        let client_file = temp_dir.path().join("client-pairings.json");
        let store = PairingStore::new(&client_file);
        // A pre-existing target (as on a rewritten store) and repeated saves
        // must never expose key material world-readable.
        fs::write(&client_file, b"[]").unwrap();
        for i in 0..3 {
            store
                .save(PairingRecord::new(
                    format!("private-{i}"),
                    "Host",
                    vec![0x11; 32],
                    0,
                ))
                .unwrap();
        }
        let entries: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|p| {
                !p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".lock"))
            })
            .collect();
        assert_eq!(
            entries,
            vec![client_file.clone()],
            "stray temporary files remain"
        );
        let mode = fs::metadata(&client_file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "store file must be private");
    }

    #[test]
    fn store_file_lock_mutual_exclusion() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_file = temp_dir.path().join("pairings.json");
        let lock1 = StoreFileLock::acquire(&store_file);
        assert!(lock1.is_ok());
        drop(lock1);
        let lock2 = StoreFileLock::acquire(&store_file);
        assert!(lock2.is_ok());
    }
}
