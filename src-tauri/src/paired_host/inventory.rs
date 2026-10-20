//! Synchronous, single-owner local daemon store. The daemon must share one instance
//! (under its mutation gate) and offload disk operations to a blocking worker.
//! No network calls, renderer state, or implicit process-global path resolution.
use crate::scoped_contracts::Epoch;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tokio::sync::watch;

const FILE_NAME: &str = "paired-hosts.v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InventoryError {
    #[error("Invalid paired host input")]
    InvalidInput,
    #[error("Paired host inventory is unavailable; original data retained")]
    Unavailable,
    #[error("Paired host inventory changed outside its owner")]
    AuthorityChanged,
    #[error("Paired host generation is stale")]
    StaleGeneration,
    #[error("Paired host requires machine authorization")]
    Unauthorized,
    #[error("Paired host generation exhausted")]
    GenerationExhausted,
    #[error("Legacy credential migration is pending")]
    MigrationPending,
}
type Result<T> = std::result::Result<T, InventoryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrantScope {
    Mirror,
    Machine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthStatus {
    Paired,
    NeedsMachineGrant,
    Revoked,
    Unknown,
}

/// The only serializable outward host projection; it cannot contain a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostView {
    pub host_id: String,
    pub relay_origin: String,
    pub machine_id: String,
    pub display_label: String,
    pub grant_scope: GrantScope,
    pub generation: Epoch,
    pub auth_status: AuthStatus,
    pub online: bool,
    /// Set only when the user explicitly links a saved SSH host to this machine.
    pub ssh_host_id: Option<String>,
}

// Deliberately no Debug or outward Serialize on inputs or credential leases.
pub struct Pairing {
    pub relay_origin: String,
    pub machine_id: String,
    pub display_label: String,
    pub grant_scope: GrantScope,
    pub device_token: String,
}

/// Caller must supply the exact explicitly host-scoped legacy key. An origin-wide
/// key or an absent machine association cannot be converted into this proof.
pub struct LegacyCredential {
    pub host_id: String,
    pub pairing: Pairing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationReceipt {
    pub host_id: String,
    pub generation: Epoch,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    relay_origin: String,
    machine_id: String,
    display_label: String,
    grant_scope: GrantScope,
    device_token: Option<String>,
    generation: Epoch,
    auth_status: AuthStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ssh_host_id: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Disk {
    version: u32,
    hosts: BTreeMap<String, Record>,
    // Only host identity/generation survives Forget; never labels or credentials.
    generations: BTreeMap<String, Epoch>,
}
impl Default for Disk {
    fn default() -> Self {
        Self {
            version: 1,
            hosts: BTreeMap::new(),
            generations: BTreeMap::new(),
        }
    }
}

pub struct CredentialLease {
    host_id: String,
    generation: Epoch,
    token: String,
    cancelled: watch::Receiver<bool>,
}
impl std::fmt::Debug for CredentialLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CredentialLease([REDACTED])")
    }
}
impl CredentialLease {
    /// Subscribe before starting IO; consumers must select cancellation against
    /// requests/sockets and validate the captured generation before adopting data.
    pub fn cancellation(&self) -> watch::Receiver<bool> {
        self.cancelled.clone()
    }
    pub fn generation(&self) -> Epoch {
        self.generation
    }
    pub fn host_id(&self) -> &str {
        &self.host_id
    }
    /// Native HTTP code alone receives the token. Never return it through IPC.
    pub fn token(&self) -> Result<&str> {
        if *self.cancelled.borrow() || self.cancelled.has_changed().is_err() {
            return Err(InventoryError::StaleGeneration);
        }
        Ok(&self.token)
    }
}

pub struct Inventory {
    path: PathBuf,
    disk: Disk,
    online: BTreeMap<String, bool>,
    cancellations: BTreeMap<String, watch::Sender<bool>>,
    fenced: bool,
    loopback_http: bool,
}
impl std::fmt::Debug for Inventory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Inventory([REDACTED])")
    }
}

pub fn normalize_origin(input: &str) -> Result<String> {
    normalize_origin_policy(input, false)
}
fn normalize_origin_policy(input: &str, loopback_http: bool) -> Result<String> {
    if input.trim() != input || input.chars().any(char::is_control) {
        return Err(InventoryError::InvalidInput);
    }
    let url = reqwest::Url::parse(input).map_err(|_| InventoryError::InvalidInput)?;
    // Reject even empty userinfo rather than letting URL canonicalization erase it.
    let authority = input
        .split_once("://")
        .ok_or(InventoryError::InvalidInput)?
        .1
        .split(['/', '?', '#'])
        .next()
        .ok_or(InventoryError::InvalidInput)?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if authority.contains('@')
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || !(url.scheme() == "https" || (loopback_http && loopback && url.scheme() == "http"))
    {
        return Err(InventoryError::InvalidInput);
    }
    Ok(url.origin().ascii_serialization())
}
#[cfg(test)]
pub(super) fn normalize_test_loopback_origin(input: &str) -> Result<String> {
    normalize_origin_policy(input, true)
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
pub fn host_key(origin: &str, machine: &str) -> Result<String> {
    if !valid_text(machine, 256) {
        return Err(InventoryError::InvalidInput);
    }
    // Match JavaScript encodeURIComponent, including its unescaped !'()* set.
    let mut encoded = String::new();
    for byte in machine.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").map_err(|_| InventoryError::InvalidInput)?;
        }
    }
    Ok(format!("{origin}/host/{encoded}"))
}
fn pairing_record(
    pairing: &Pairing,
    generation: Epoch,
    loopback_http: bool,
) -> Result<(String, Record)> {
    let origin = normalize_origin_policy(&pairing.relay_origin, loopback_http)?;
    if !valid_text(&pairing.display_label, 256)
        || !valid_text(&pairing.device_token, 8192)
        || !pairing
            .device_token
            .bytes()
            .all(|b| (33..=126).contains(&b))
    {
        return Err(InventoryError::InvalidInput);
    }
    let id = host_key(&origin, &pairing.machine_id)?;
    Ok((
        id,
        Record {
            relay_origin: origin,
            machine_id: pairing.machine_id.clone(),
            display_label: pairing.display_label.clone(),
            grant_scope: pairing.grant_scope,
            device_token: Some(pairing.device_token.clone()),
            generation,
            auth_status: match pairing.grant_scope {
                GrantScope::Machine => AuthStatus::Paired,
                GrantScope::Mirror => AuthStatus::NeedsMachineGrant,
            },
            ssh_host_id: None,
        },
    ))
}
fn load(path: &Path, loopback_http: bool) -> Result<Disk> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Disk::default()),
        Err(_) => return Err(InventoryError::Unavailable),
    };
    // Never include serde errors: malformed string values can contain credentials.
    let disk: Disk = serde_json::from_slice(&bytes).map_err(|_| InventoryError::Unavailable)?;
    if disk.version != 1 {
        return Err(InventoryError::Unavailable);
    }
    for (id, generation) in &disk.generations {
        if generation.0 == 0 || !valid_text(id, 4096) {
            return Err(InventoryError::Unavailable);
        }
    }
    for (id, row) in &disk.hosts {
        if normalize_origin_policy(&row.relay_origin, loopback_http)
            .ok()
            .as_ref()
            != Some(&row.relay_origin)
            || host_key(&row.relay_origin, &row.machine_id).ok().as_ref() != Some(id)
            || !valid_text(&row.display_label, 256)
            || disk.generations.get(id) != Some(&row.generation)
        {
            return Err(InventoryError::Unavailable);
        }
        match (&row.device_token, row.auth_status, row.grant_scope) {
            (Some(token), AuthStatus::Paired, GrantScope::Machine)
            | (Some(token), AuthStatus::NeedsMachineGrant, GrantScope::Mirror)
                if valid_text(token, 8192) && token.bytes().all(|b| (33..=126).contains(&b)) => {}
            (None, AuthStatus::Revoked | AuthStatus::Unknown, _) => {}
            _ => return Err(InventoryError::Unavailable),
        }
    }
    Ok(disk)
}

impl Inventory {
    pub fn open(private_data_dir: &Path) -> Result<Self> {
        Self::open_policy(private_data_dir, false)
    }
    /// Isolated native fixtures only; not compiled into production binaries.
    #[cfg(test)]
    pub(crate) fn open_test_loopback(private_data_dir: &Path) -> Result<Self> {
        Self::open_policy(private_data_dir, true)
    }
    fn open_policy(private_data_dir: &Path, loopback_http: bool) -> Result<Self> {
        let path = private_data_dir.join(FILE_NAME);
        let disk = load(&path, loopback_http)?;
        Ok(Self {
            path,
            disk,
            online: BTreeMap::new(),
            cancellations: BTreeMap::new(),
            fenced: false,
            loopback_http,
        })
    }
    pub fn list(&self) -> Vec<HostView> {
        self.disk
            .hosts
            .iter()
            .map(|(id, row)| HostView {
                host_id: id.clone(),
                relay_origin: row.relay_origin.clone(),
                machine_id: row.machine_id.clone(),
                display_label: row.display_label.clone(),
                grant_scope: row.grant_scope,
                generation: row.generation,
                auth_status: if self.fenced {
                    AuthStatus::Unknown
                } else {
                    row.auth_status
                },
                online: !self.fenced && self.online.get(id).copied().unwrap_or(true),
                ssh_host_id: row.ssh_host_id.clone(),
            })
            .collect()
    }
    /// Durable exact-generation verification before desktop removes its legacy copy.
    pub fn read_verified(&mut self, id: &str, generation: Epoch) -> Result<HostView> {
        self.validate_generation(id, generation)?;
        if load(&self.path, self.loopback_http).ok().as_ref() != Some(&self.disk) {
            self.fence();
            return Err(InventoryError::Unavailable);
        }
        self.list()
            .into_iter()
            .find(|row| row.host_id == id)
            .ok_or(InventoryError::StaleGeneration)
    }
    /// Capture live and forgotten identities before an issuer resolves a PIN.
    pub fn generation_snapshot(&self) -> Result<BTreeMap<String, Epoch>> {
        if self.fenced {
            return Err(InventoryError::Unavailable);
        }
        Ok(self.disk.generations.clone())
    }
    pub fn validate_generation(&self, id: &str, generation: Epoch) -> Result<()> {
        if self.fenced {
            return Err(InventoryError::Unavailable);
        }
        if self
            .disk
            .hosts
            .get(id)
            .is_none_or(|row| row.generation != generation)
        {
            return Err(InventoryError::StaleGeneration);
        }
        Ok(())
    }
    pub fn set_online(&mut self, id: &str, generation: Epoch, online: bool) -> Result<()> {
        self.validate_generation(id, generation)?;
        self.online.insert(id.to_owned(), online);
        Ok(())
    }
    /// Links or unlinks a saved SSH host for this machine. Nothing infers an SSH target from a
    /// hostname or scans for one: only an explicit caller action sets this.
    pub fn set_ssh_host_id(
        &mut self,
        id: &str,
        generation: Epoch,
        ssh_host_id: Option<String>,
    ) -> Result<()> {
        self.validate_generation(id, generation)?;
        let mut candidate = self.disk.clone();
        candidate
            .hosts
            .get_mut(id)
            .ok_or(InventoryError::StaleGeneration)?
            .ssh_host_id = ssh_host_id;
        self.commit(candidate, id)
    }
    pub fn capture(&mut self, id: &str, generation: Epoch) -> Result<CredentialLease> {
        self.validate_generation(id, generation)?;
        let row = &self.disk.hosts[id];
        if row.auth_status != AuthStatus::Paired {
            return Err(InventoryError::Unauthorized);
        }
        let token = row
            .device_token
            .clone()
            .ok_or(InventoryError::Unauthorized)?;
        let sender = self
            .cancellations
            .entry(id.to_owned())
            .or_insert_with(|| watch::channel(false).0);
        Ok(CredentialLease {
            host_id: id.to_owned(),
            generation,
            token,
            cancelled: sender.subscribe(),
        })
    }
    fn invalidate(&mut self, id: &str) {
        if let Some(sender) = self.cancellations.remove(id) {
            sender.send_replace(true);
        }
        self.online.remove(id);
    }
    fn fence(&mut self) {
        self.fenced = true;
        for (_, sender) in std::mem::take(&mut self.cancellations) {
            sender.send_replace(true);
        }
    }
    fn commit(&mut self, candidate: Disk, id: &str) -> Result<()> {
        if self.fenced {
            return Err(InventoryError::Unavailable);
        }
        // Detect external replacement/corruption before allowing any overwrite.
        if load(&self.path, self.loopback_http).ok().as_ref() != Some(&self.disk) {
            self.fence();
            return Err(InventoryError::AuthorityChanged);
        }
        if crate::remote::auth::write_private_json(&self.path, &candidate).is_err() {
            // The shared writer may have reached rename: ambiguity must fail closed.
            self.fence();
            return Err(InventoryError::Unavailable);
        }
        if load(&self.path, self.loopback_http).ok().as_ref() != Some(&candidate) {
            self.fence();
            return Err(InventoryError::Unavailable);
        }
        self.disk = candidate;
        self.invalidate(id);
        Ok(())
    }
    fn next_generation(&self, id: &str) -> Result<Epoch> {
        self.disk
            .generations
            .get(id)
            .map_or(0, |v| v.0)
            .checked_add(1)
            .map(Epoch)
            .ok_or(InventoryError::GenerationExhausted)
    }
    /// Accept only an issuer-verified pairing result, not a renderer-supplied scope.
    pub fn pair(&mut self, pairing: &Pairing) -> Result<HostView> {
        let (id, mut row) = pairing_record(pairing, Epoch(1), self.loopback_http)?;
        row.generation = self.next_generation(&id)?;
        let mut candidate = self.disk.clone();
        candidate.generations.insert(id.clone(), row.generation);
        candidate.hosts.insert(id.clone(), row);
        self.commit(candidate, &id)?;
        self.list()
            .into_iter()
            .find(|row| row.host_id == id)
            .ok_or(InventoryError::Unavailable)
    }
    pub fn forget(&mut self, id: &str, generation: Epoch) -> Result<()> {
        self.validate_generation(id, generation)?;
        let mut candidate = self.disk.clone();
        candidate.hosts.remove(id);
        candidate
            .generations
            .insert(id.to_owned(), self.next_generation(id)?);
        self.commit(candidate, id)
    }
    /// Auth failure must name a captured generation; an old 401 cannot revoke a re-pair.
    pub fn mark_auth_unavailable(
        &mut self,
        id: &str,
        generation: Epoch,
        revoked: bool,
    ) -> Result<()> {
        self.validate_generation(id, generation)?;
        let mut candidate = self.disk.clone();
        let next = self.next_generation(id)?;
        let row = candidate
            .hosts
            .get_mut(id)
            .ok_or(InventoryError::StaleGeneration)?;
        row.generation = next;
        row.device_token = None;
        row.auth_status = if revoked {
            AuthStatus::Revoked
        } else {
            AuthStatus::Unknown
        };
        candidate.generations.insert(id.to_owned(), next);
        self.commit(candidate, id)
    }
    /// Copy only. Receipt permits a later desktop lane to delete its exact legacy key.
    /// Never overwrites a different native credential or infers origin-wide ownership.
    pub fn migrate_copy(&mut self, legacy: &LegacyCredential) -> Result<MigrationReceipt> {
        let (id, mut record) = pairing_record(&legacy.pairing, Epoch(1), self.loopback_http)
            .map_err(|_| InventoryError::MigrationPending)?;
        if legacy.host_id != id || self.fenced {
            return Err(InventoryError::MigrationPending);
        }
        if let Some(existing) = self.disk.hosts.get(&id) {
            record.generation = existing.generation;
            if existing != &record {
                return Err(InventoryError::MigrationPending);
            }
        } else {
            // A forgotten native identity must not be resurrected from old browser data.
            if self.disk.generations.contains_key(&id) {
                return Err(InventoryError::MigrationPending);
            }
            self.pair(&legacy.pairing)
                .map_err(|_| InventoryError::MigrationPending)?;
        }
        let verified =
            load(&self.path, self.loopback_http).map_err(|_| InventoryError::MigrationPending)?;
        if verified != self.disk {
            self.fence();
            return Err(InventoryError::MigrationPending);
        }
        Ok(MigrationReceipt {
            host_id: id.clone(),
            generation: self.disk.hosts[&id].generation,
        })
    }
}
