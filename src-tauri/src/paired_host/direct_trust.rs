//! Peer keys for the direct (P2P) path.
//!
//! The relay is UNTRUSTED: a peer key is never learned from an offer or an answer.
//! Keys reach this store only through:
//! - [`DirectTrustStore::provision`] / [`DirectTrustStore::revoke`]: an explicit user action
//!   (`ferryx direct-trust`), recorded as `source: "manual"`; and
//! - [`DirectTrustStore::provision_from_account`]: a key taken from an account-signed peer record
//!   (`account::peer_record`) for a same-owner machine, recorded as `source: "account"`.
//!
//! Invariants: a manual pin always wins over an account key; an account key may only change for a
//! strictly newer enrollment epoch; a revoked machine id is tombstoned and never auto-repinned
//! until the user provisions it manually again.
//!
//! On disk: version 2 `{version:2, peers:{id:{key,source,epoch?}}, revoked:[id..]}`. Version 1
//! files (`{version:1, peers:{id:key}}`) are still read as manual pins; every write emits v2.
//!
//! A machine without a key stays on the relay. All methods here do synchronous disk IO; async
//! callers go through [`trusted_key`] / [`provision`] / [`provision_from_account`], which run on
//! `crate::ipc::run_blocking`.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const TRUST_FILE: &str = "direct-trust.json";
const MAX_MACHINE_ID: usize = 128;

/// Set to `0` to disable account-attested automatic pinning on this machine (default on).
pub const AUTO_TRUST_ENV: &str = "FERRYX_DIRECT_AUTO_TRUST";

/// Whether account-attested automatic pinning is enabled: default on, turned off by the env
/// opt-out or by the user's persisted "disable auto-trust" setting.
pub fn auto_trust_enabled() -> bool {
    auto_trust_enabled_from(std::env::var(AUTO_TRUST_ENV).ok().as_deref())
        && !crate::remote::security_settings::load().disable_auto_trust
}

pub fn auto_trust_enabled_from(value: Option<&str>) -> bool {
    !matches!(value.map(str::trim), Some("0") | Some("false") | Some("off") | Some("no"))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrustError {
    #[error("invalid machine id")]
    InvalidMachineId,
    #[error("invalid ed25519 public key")]
    InvalidKey,
    #[error("direct trust store unavailable: {0}")]
    Io(String),
    #[error("direct trust store is corrupt")]
    Corrupt,
    #[error("a different key is already pinned for this machine; pass --replace")]
    PinConflict,
    #[error("a manual pin exists for this machine; account keys never override it")]
    ManualPinWins,
    #[error("account key change requires a strictly newer enrollment epoch")]
    StaleEpoch,
    #[error("this machine was revoked; only a manual provision can trust it again")]
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PinSource {
    Manual,
    Account,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pin {
    /// Standard base64 32-byte ed25519 verifying key.
    key: String,
    source: PinSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    epoch: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Disk {
    version: u32,
    peers: BTreeMap<String, Pin>,
    #[serde(default)]
    revoked: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiskV1 {
    version: u32,
    peers: BTreeMap<String, String>,
}

impl Default for Disk {
    fn default() -> Self {
        Self {
            version: 2,
            peers: BTreeMap::new(),
            revoked: BTreeSet::new(),
        }
    }
}

/// Serializes read-modify-write cycles inside this process.
static WRITE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectTrustStore {
    path: PathBuf,
}

impl DirectTrustStore {
    pub fn at(dir: &Path) -> Self {
        Self {
            path: dir.join(TRUST_FILE),
        }
    }

    /// `<data>/remote/direct-trust.json`, beside `identity.json`.
    pub fn canonical() -> Result<Self, TrustError> {
        crate::remote::auth::canonical_remote_dir()
            .map(|dir| Self::at(&dir))
            .ok_or_else(|| TrustError::Io("cannot resolve remote data dir".into()))
    }

    fn read(&self) -> Result<Disk, TrustError> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Disk::default()),
            Err(error) => return Err(TrustError::Io(error.to_string())),
        };
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| TrustError::Corrupt)?;
        match value.get("version").and_then(serde_json::Value::as_u64) {
            Some(1) => {
                let v1: DiskV1 = serde_json::from_value(value).map_err(|_| TrustError::Corrupt)?;
                debug_assert_eq!(v1.version, 1);
                Ok(Disk {
                    version: 2,
                    peers: v1
                        .peers
                        .into_iter()
                        .map(|(id, key)| (id, Pin { key, source: PinSource::Manual, epoch: None }))
                        .collect(),
                    revoked: BTreeSet::new(),
                })
            }
            Some(2) => serde_json::from_value(value).map_err(|_| TrustError::Corrupt),
            _ => Err(TrustError::Corrupt),
        }
    }

    fn write(&self, disk: &Disk) -> Result<(), TrustError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| TrustError::Io(e.to_string()))?;
        }
        crate::remote::auth::write_private_json(&self.path, disk)
            .map_err(|e| TrustError::Io(e.to_string()))
    }

    /// Explicitly (manually) trust `machine_id`. An existing different pin is only overwritten
    /// when `replace` is set (the user re-keyed that machine). An account-sourced pin with the
    /// same key is upgraded to manual. A manual provision clears a revocation tombstone.
    pub fn provision(&self, machine_id: &str, public_key_b64: &str, replace: bool) -> Result<(), TrustError> {
        validate_machine_id(machine_id)?;
        let key = STANDARD.encode(parse_verifying_key(public_key_b64)?.to_bytes());
        let _guard = WRITE_LOCK.lock();
        let mut disk = self.read()?;
        match disk.peers.get(machine_id) {
            Some(existing)
                if existing.key == key
                    && existing.source == PinSource::Manual
                    && !disk.revoked.contains(machine_id) =>
            {
                return Ok(())
            }
            Some(existing) if existing.key != key && !replace => return Err(TrustError::PinConflict),
            _ => {}
        }
        disk.revoked.remove(machine_id);
        disk.peers.insert(machine_id.to_owned(), Pin { key, source: PinSource::Manual, epoch: None });
        self.write(&disk)
    }

    /// Pins a key taken from a verified account peer record. Never overrides a manual pin,
    /// accepts a key change only for a strictly newer `epoch`, and refuses tombstoned ids.
    /// Returns `Ok(())` when the key is (now) pinned from the account.
    pub fn provision_from_account(
        &self,
        machine_id: &str,
        public_key_b64: &str,
        epoch: u64,
    ) -> Result<(), TrustError> {
        validate_machine_id(machine_id)?;
        let key = STANDARD.encode(parse_verifying_key(public_key_b64)?.to_bytes());
        let _guard = WRITE_LOCK.lock();
        let mut disk = self.read()?;
        if disk.revoked.contains(machine_id) {
            return Err(TrustError::Revoked);
        }
        match disk.peers.get(machine_id) {
            Some(existing) if existing.source == PinSource::Manual => {
                return if existing.key == key { Ok(()) } else { Err(TrustError::ManualPinWins) };
            }
            Some(existing) if existing.key == key => {
                if existing.epoch.is_some_and(|known| known >= epoch) {
                    return Ok(());
                }
            }
            Some(existing) if existing.epoch.is_some_and(|known| epoch <= known) => {
                return Err(TrustError::StaleEpoch);
            }
            _ => {}
        }
        disk.peers.insert(
            machine_id.to_owned(),
            Pin { key, source: PinSource::Account, epoch: Some(epoch) },
        );
        self.write(&disk)
    }

    pub fn list(&self) -> Result<Vec<(String, String)>, TrustError> {
        Ok(self.read()?.peers.into_iter().map(|(id, pin)| (id, pin.key)).collect())
    }

    /// Removes trust and tombstones the id so account records cannot re-pin it;
    /// returns whether a key was present.
    pub fn revoke(&self, machine_id: &str) -> Result<bool, TrustError> {
        validate_machine_id(machine_id)?;
        let _guard = WRITE_LOCK.lock();
        let mut disk = self.read()?;
        let removed = disk.peers.remove(machine_id).is_some();
        let tombstoned = disk.revoked.insert(machine_id.to_owned());
        if removed || tombstoned {
            self.write(&disk)?;
        }
        Ok(removed)
    }

    /// `Ok(None)` means "not trusted": the caller must stay on the relay.
    pub fn trusted_key(&self, machine_id: &str) -> Result<Option<VerifyingKey>, TrustError> {
        validate_machine_id(machine_id)?;
        match self.read()?.peers.get(machine_id) {
            Some(pin) => parse_verifying_key(&pin.key).map(Some),
            None => Ok(None),
        }
    }
}

pub fn validate_machine_id(machine_id: &str) -> Result<(), TrustError> {
    let ok = !machine_id.is_empty()
        && machine_id.len() <= MAX_MACHINE_ID
        && machine_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
    ok.then_some(()).ok_or(TrustError::InvalidMachineId)
}

pub fn parse_verifying_key(public_key_b64: &str) -> Result<VerifyingKey, TrustError> {
    let bytes: [u8; 32] = STANDARD
        .decode(public_key_b64.trim())
        .map_err(|_| TrustError::InvalidKey)?
        .try_into()
        .map_err(|_| TrustError::InvalidKey)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| TrustError::InvalidKey)
}

/// Async lookup on a blocking worker. Any failure (corrupt store, IO) is reported
/// as "not trusted" to the route layer, which keeps the relay; the error is logged.
pub async fn trusted_key(store: DirectTrustStore, machine_id: String) -> Option<VerifyingKey> {
    let result = crate::ipc::run_blocking(move || {
        store
            .trusted_key(&machine_id)
            .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
    })
    .await;
    match result {
        Ok(key) => key,
        Err(error) => {
            tracing::warn!("direct trust lookup failed; staying on relay: {error:?}");
            None
        }
    }
}

/// Async explicit provisioning entry point (IPC callers).
pub async fn provision(
    store: DirectTrustStore,
    machine_id: String,
    public_key_b64: String,
    replace: bool,
) -> Result<(), crate::ipc::IpcError> {
    crate::ipc::run_blocking(move || {
        store
            .provision(&machine_id, &public_key_b64, replace)
            .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
    })
    .await
}

/// Async account-attested pinning (verified peer record). Errors are typed for the caller to log.
pub async fn provision_from_account(
    store: DirectTrustStore,
    machine_id: String,
    public_key_b64: String,
    epoch: u64,
) -> Result<(), TrustError> {
    crate::ipc::run_blocking(move || {
        Ok(store.provision_from_account(&machine_id, &public_key_b64, epoch))
    })
    .await
    .map_err(|error| TrustError::Io(error.message))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_b64(seed: u8) -> String {
        let key = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        STANDARD.encode(key.verifying_key().to_bytes())
    }

    #[test]
    fn absent_store_trusts_nobody() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        assert_eq!(store.trusted_key("host-a").unwrap(), None);
    }

    #[test]
    fn provision_then_revoke_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        store.provision("host-a", &key_b64(3), false).unwrap();
        let expected = parse_verifying_key(&key_b64(3)).unwrap();
        assert_eq!(store.trusted_key("host-a").unwrap(), Some(expected));
        assert_eq!(store.trusted_key("host-b").unwrap(), None);
        assert!(store.revoke("host-a").unwrap());
        assert_eq!(store.trusted_key("host-a").unwrap(), None);
    }

    #[test]
    fn malformed_inputs_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        assert_eq!(store.provision("", &key_b64(1), false), Err(TrustError::InvalidMachineId));
        assert_eq!(store.provision("a/b", &key_b64(1), false), Err(TrustError::InvalidMachineId));
        assert_eq!(store.provision("host", "not-base64!", false), Err(TrustError::InvalidKey));
        assert_eq!(
            store.provision("host", &STANDARD.encode([1u8; 31]), false),
            Err(TrustError::InvalidKey)
        );
    }

    #[test]
    fn a_different_pin_needs_explicit_replace() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        store.provision("host", &key_b64(1), false).unwrap();
        store.provision("host", &key_b64(1), false).unwrap();
        assert_eq!(store.provision("host", &key_b64(2), false), Err(TrustError::PinConflict));
        store.provision("host", &key_b64(2), true).unwrap();
        assert_eq!(
            store.trusted_key("host").unwrap(),
            Some(parse_verifying_key(&key_b64(2)).unwrap())
        );
    }

    #[test]
    fn unknown_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRUST_FILE), br#"{"version":3,"peers":{}}"#).unwrap();
        assert_eq!(DirectTrustStore::at(dir.path()).trusted_key("h"), Err(TrustError::Corrupt));
    }

    #[test]
    fn corrupt_store_is_an_error_not_trust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRUST_FILE), b"{not json").unwrap();
        let store = DirectTrustStore::at(dir.path());
        assert_eq!(store.trusted_key("host-a"), Err(TrustError::Corrupt));
    }

    #[test]
    fn v1_file_still_reads() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = format!(r#"{{"version":1,"peers":{{"host-a":"{}"}}}}"#, key_b64(4));
        std::fs::write(dir.path().join(TRUST_FILE), v1).unwrap();
        let store = DirectTrustStore::at(dir.path());
        let expected = parse_verifying_key(&key_b64(4)).unwrap();
        assert_eq!(store.trusted_key("host-a").unwrap(), Some(expected));
        // A v1 pin is manual: account keys never override it.
        assert_eq!(
            store.provision_from_account("host-a", &key_b64(5), 9),
            Err(TrustError::ManualPinWins)
        );
        // Any write upgrades the file to v2 and keeps the pin.
        store.provision("host-b", &key_b64(6), false).unwrap();
        let disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(TRUST_FILE)).unwrap()).unwrap();
        assert_eq!(disk["version"], 2);
        assert_eq!(disk["peers"]["host-a"]["source"], "manual");
        assert_eq!(store.trusted_key("host-a").unwrap(), Some(expected));
    }

    #[test]
    fn manual_pin_beats_account() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        store.provision("host", &key_b64(1), false).unwrap();
        assert_eq!(
            store.provision_from_account("host", &key_b64(2), 100),
            Err(TrustError::ManualPinWins)
        );
        assert_eq!(store.provision_from_account("host", &key_b64(1), 100), Ok(()));
        assert_eq!(
            store.trusted_key("host").unwrap(),
            Some(parse_verifying_key(&key_b64(1)).unwrap())
        );
        // A manual provision over an account pin with a different key still needs --replace.
        store.provision_from_account("other", &key_b64(3), 1).unwrap();
        assert_eq!(store.provision("other", &key_b64(4), false), Err(TrustError::PinConflict));
        store.provision("other", &key_b64(3), false).unwrap();
        assert_eq!(
            store.provision_from_account("other", &key_b64(4), 2),
            Err(TrustError::ManualPinWins),
            "re-provisioning the same key manually upgrades the pin to manual"
        );
    }

    #[test]
    fn account_key_change_needs_newer_epoch() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        store.provision_from_account("host", &key_b64(1), 5).unwrap();
        assert_eq!(store.provision_from_account("host", &key_b64(2), 5), Err(TrustError::StaleEpoch));
        assert_eq!(store.provision_from_account("host", &key_b64(2), 4), Err(TrustError::StaleEpoch));
        assert_eq!(
            store.trusted_key("host").unwrap(),
            Some(parse_verifying_key(&key_b64(1)).unwrap())
        );
        // Same key at an older epoch is a no-op, not a downgrade.
        store.provision_from_account("host", &key_b64(1), 3).unwrap();
        store.provision_from_account("host", &key_b64(2), 6).unwrap();
        assert_eq!(
            store.trusted_key("host").unwrap(),
            Some(parse_verifying_key(&key_b64(2)).unwrap())
        );
    }

    #[test]
    fn revoked_peer_not_auto_repinned() {
        let dir = tempfile::tempdir().unwrap();
        let store = DirectTrustStore::at(dir.path());
        store.provision_from_account("host", &key_b64(1), 1).unwrap();
        assert!(store.revoke("host").unwrap());
        assert_eq!(store.provision_from_account("host", &key_b64(1), 2), Err(TrustError::Revoked));
        assert_eq!(store.trusted_key("host").unwrap(), None);
        // Revoking an id that was never pinned still tombstones it.
        assert!(!store.revoke("never").unwrap());
        assert_eq!(store.provision_from_account("never", &key_b64(2), 1), Err(TrustError::Revoked));
        // A manual provision clears the tombstone.
        store.provision("host", &key_b64(1), false).unwrap();
        assert!(store.trusted_key("host").unwrap().is_some());
    }

    #[test]
    fn auto_trust_env_opt_out() {
        assert!(auto_trust_enabled_from(None));
        assert!(auto_trust_enabled_from(Some("1")));
        assert!(!auto_trust_enabled_from(Some("0")));
        assert!(!auto_trust_enabled_from(Some(" false ")));
    }
}
