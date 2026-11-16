//! The sole writer of ssh_tunnels.json. All read/modify/write transactions hold
//! the sidecar lock; callers acquire it BEFORE the runtime registry mutex and
//! release it BEFORE awaiting reapers. Run synchronous operations on run_blocking.
//! Windows tunnel commands refuse before reaching this module; no Windows lock
//! primitive is implemented here.
use super::tunnel::{validate_saved_spec, validate_specs_for_host, TunnelHostIdentity, TunnelSpec};
use crate::ipc::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

pub trait Clock: Send + Sync {
    fn now_unix_ms(&self) -> u64;
}
#[derive(Debug, Default)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|t| t.as_millis().min(u64::MAX as u128) as u64).unwrap_or(0)
    }
}
#[cfg(test)]
pub struct TestClock(std::sync::atomic::AtomicU64);
#[cfg(test)]
impl TestClock {
    pub fn new(now: u64) -> Self { Self(std::sync::atomic::AtomicU64::new(now)) }
    pub fn set(&mut self, now: u64) { self.0.store(now, std::sync::atomic::Ordering::SeqCst); }
}
#[cfg(test)]
impl Clock for TestClock {
    fn now_unix_ms(&self) -> u64 { self.0.load(std::sync::atomic::Ordering::SeqCst) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatus {
    pub kind: String,
    pub invalid_hosts: Vec<String>,
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostIdentityEntry {
    pub revision: u64,
    pub fingerprint: TunnelHostIdentity,
    /// None until the detached teardown completes. Never serialized as null.
    pub recorded_at_unix_ms: Option<u64>,
    pub pending_since_disk: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskIdentity {
    revision: u64,
    fingerprint: TunnelHostIdentity,
    recorded_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TunnelStore {
    pub version: u64,
    pub definitions_revision: u64,
    pub host_revisions: HashMap<String, u64>,
    pub host_identities: HashMap<String, HostIdentityEntry>,
    pub hosts: HashMap<String, Vec<TunnelSpec>>,
    pub invalid_rows: HashMap<String, Vec<String>>,
    pub host_has_invalid_definitions: Vec<String>,
    pub store_status: StoreStatus,
    pub diagnostics: Vec<String>,
    /// Persisted on reset so unknown hosts cannot fall back to revision zero.
    pub revision_floor: u64,
    // Non-array host values need to retain their original shape as well as bytes.
    invalid_host_values: HashMap<String, String>,
    // Unknown envelope fields must survive an older build's writes.
    extra: HashMap<String, Value>,
}
impl Default for TunnelStore {
    fn default() -> Self {
        Self { version: 1, definitions_revision: 0, host_revisions: HashMap::new(), host_identities: HashMap::new(), hosts: HashMap::new(), invalid_rows: HashMap::new(), host_has_invalid_definitions: Vec::new(), store_status: StoreStatus { kind: "ok".into(), invalid_hosts: Vec::new(), path: String::new() }, diagnostics: Vec::new(), revision_floor: 0, invalid_host_values: HashMap::new(), extra: HashMap::new() }
    }
}
impl TunnelStore {
    pub fn host_revision(&self, host_id: &str) -> u64 {
        self.host_revisions.get(host_id).copied().unwrap_or(0).max(self.revision_floor)
    }
    pub fn active_host_identity(&self, host_id: &str) -> Option<&HostIdentityEntry> {
        self.host_identities.get(host_id).filter(|entry| entry.revision == self.host_revision(host_id))
    }
    /// Seed corrupt-boot/reset authority from unix seconds, strictly above all
    /// known revisions. Keep this floor for the process lifetime on corrupt boot.
    pub fn seed_floor(&mut self, now: u64) -> u64 {
        let maximum = self.host_revisions.values().copied().chain(self.host_identities.values().map(|entry| entry.revision)).max().unwrap_or(0).max(self.revision_floor);
        self.revision_floor = now.max(maximum.saturating_add(1));
        self.revision_floor
    }
    /// Update memory first, even if the subsequent disk write cannot succeed.
    pub fn bump_host_revision(&mut self, host_id: &str, fingerprint: TunnelHostIdentity, now_ms: u64) -> Result<u64, IpcError> {
        if now_ms == u64::MAX { return Err(argument("recordedAtUnixMs")); }
        let revision = self.host_revision(host_id).checked_add(1).ok_or_else(|| IpcError::internal("Tunnel host revision exhausted"))?;
        self.host_revisions.insert(host_id.into(), revision);
        self.host_identities.insert(host_id.into(), HostIdentityEntry { revision, fingerprint, recorded_at_unix_ms: None, pending_since_disk: now_ms });
        Ok(revision)
    }
    pub fn mark_identity_pending(&mut self, host_id: &str, now_ms: u64) {
        if let Some(entry) = self.host_identities.get_mut(host_id) {
            entry.recorded_at_unix_ms = None;
            entry.pending_since_disk = now_ms.min(u64::MAX - 1);
        }
    }
    /// An old detached reaper must never stamp a newer invalidation's entry.
    pub fn stamp_identity(&mut self, host_id: &str, rev: u64, now_ms: u64) {
        if let Some(entry) = self.host_identities.get_mut(host_id).filter(|entry| entry.revision == rev) {
            entry.recorded_at_unix_ms = Some(now_ms.min(u64::MAX - 1));
        }
    }
    fn refresh_status(&mut self) {
        let mut hosts: Vec<_> = self.invalid_rows.keys().cloned().collect(); hosts.sort();
        self.host_has_invalid_definitions = hosts.clone();
        self.store_status.invalid_hosts = hosts;
        self.store_status.kind = if self.invalid_rows.is_empty() { "ok" } else { "partial" }.into();
    }
    fn writable(&self, authority: bool) -> Result<(), IpcError> {
        match self.store_status.kind.as_str() {
            "corrupt" => Err(store_error(&self.store_status.path, "corrupt_store")),
            "unsupported_version" => Err(store_error(&self.store_status.path, if authority { "corrupt_store" } else { "unsupported_version" })),
            _ => Ok(()),
        }
    }
    /// Reconcile disk and process authority without changing definitions_revision.
    fn merge_authority(&mut self, memory: &Self) {
        self.revision_floor = self.revision_floor.max(memory.revision_floor);
        for (host, rev) in &memory.host_revisions {
            let value = self.host_revisions.entry(host.clone()).or_default(); *value = (*value).max(*rev);
        }
        for (host, incoming) in &memory.host_identities {
            if incoming.revision != self.host_revision(host) { continue; }
            let replace = self.host_identities.get(host).is_none_or(|disk| {
                incoming.revision > disk.revision || (incoming.revision == disk.revision && match (incoming.recorded_at_unix_ms, disk.recorded_at_unix_ms) {
                    // Pending memory is authoritative until its own reaper stamps it.
                    (None, _) => true,
                    (Some(_), None) => true,
                    (Some(a), Some(b)) => a >= b,
                })
            });
            if replace { self.host_identities.insert(host.clone(), incoming.clone()); }
        }
    }
}

fn argument(field: &str) -> IpcError {
    IpcError::new(IpcErrorCode::InvalidArgument, "Invalid tunnel store request").with_details(json!({"field": field}))
}
fn store_error(path: &str, reason: &str) -> IpcError {
    IpcError::new(IpcErrorCode::Unsupported, "Tunnel store cannot be written").with_details(json!({"reason": reason, "path": path}))
}
fn io_error(error: std::io::Error) -> IpcError {
    IpcError::new(IpcErrorCode::IoError, format!("Tunnel store I/O failed: {error}"))
}

/// Mirror the host path's FERRYX_DATA_DIR and dev-directory routing without
/// depending on or changing the host store's reader/writer.
pub fn store_path(app_data_dir: &Path, dev_runtime: bool) -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR") { return PathBuf::from(dir).join("ssh_tunnels.json"); }
    if dev_runtime { app_data_dir.join("dev").join("ssh_tunnels.json") } else { app_data_dir.join("ssh_tunnels.json") }
}
#[cfg(feature = "desktop")]
pub fn get_tunnel_store_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, IpcError> {
    use tauri::Manager;
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR") { return Ok(PathBuf::from(dir).join("ssh_tunnels.json")); }
    let directory = app.path().app_data_dir().map_err(|e| IpcError::new(IpcErrorCode::IoError, format!("Failed to resolve app data dir: {e}")))?;
    Ok(store_path(&directory, crate::daemon::server::is_dev_runtime()))
}

// serde_json's raw_value feature is not enabled in this crate. Stream byte
// offsets retain each undecodable JSON value's exact lexical representation,
// including whitespace INSIDE it, without adding a dependency or feature.
fn raw_value(input: &str) -> Result<(&str, &str), serde_json::Error> {
    let input = input.trim_start();
    let mut stream = serde_json::Deserializer::from_str(input).into_iter::<Value>();
    match stream.next() { Some(value) => { value?; }, None => { return serde_json::from_str::<Value>("").map(|_| ("", "")); } }
    let end = stream.byte_offset(); Ok((&input[..end], &input[end..]))
}
fn object_entries(input: &str) -> Result<Vec<(String, String)>, serde_json::Error> {
    let mut rest = input.trim_start().strip_prefix('{').unwrap_or("").trim_start();
    let mut entries = Vec::new();
    while !rest.starts_with('}') && !rest.is_empty() {
        let (key, tail) = raw_value(rest)?;
        let key: String = serde_json::from_str(key)?;
        let (value, tail) = raw_value(tail.trim_start().strip_prefix(':').unwrap_or(""))?;
        entries.push((key, value.to_owned()));
        rest = tail.trim_start();
        if let Some(tail) = rest.strip_prefix(',') { rest = tail.trim_start(); } else { break; }
    }
    Ok(entries)
}
fn array_rows(input: &str) -> Result<Vec<String>, serde_json::Error> {
    let mut rest = input.trim_start().strip_prefix('[').unwrap_or("").trim_start();
    let mut rows = Vec::new();
    while !rest.starts_with(']') && !rest.is_empty() {
        let (row, tail) = raw_value(rest)?; rows.push(row.into()); rest = tail.trim_start();
        if let Some(tail) = rest.strip_prefix(',') { rest = tail.trim_start(); } else { break; }
    }
    Ok(rows)
}

pub fn load_store(path: &Path, now_ms: u64) -> Result<TunnelStore, IpcError> {
    let mut store = TunnelStore::default(); store.store_status.path = path.to_string_lossy().into_owned();
    let bytes = match fs::read(path) { Ok(bytes) => bytes, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(store), Err(e) => return Err(io_error(e)) };
    let decode = || -> Result<TunnelStore, String> {
        let input = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        let root = value.as_object().ok_or("Tunnel store must be an object")?;
        let mut result = store.clone();
        result.version = root.get("version").and_then(Value::as_u64).ok_or("Missing tunnel store version")?;
        if result.version > 1 { result.store_status.kind = "unsupported_version".into(); result.diagnostics.push("Tunnel store version is newer than this build".into()); return Ok(result); }
        if result.version != 1 { return Err("Invalid tunnel store version".into()); }
        result.definitions_revision = root.get("definitions_revision").map(|v| serde_json::from_value(v.clone())).transpose().map_err(|e| e.to_string())?.unwrap_or(0);
        result.revision_floor = root.get("revision_floor").map(|v| serde_json::from_value(v.clone())).transpose().map_err(|e| e.to_string())?.unwrap_or(0);
        result.host_revisions = root.get("host_revisions").map(|v| serde_json::from_value(v.clone())).transpose().map_err(|e| e.to_string())?.unwrap_or_default();
        let identities: HashMap<String, DiskIdentity> = root.get("host_identities").map(|v| serde_json::from_value(v.clone())).transpose().map_err(|e| e.to_string())?.unwrap_or_default();
        for (host, entry) in identities {
            if entry.recorded_at_unix_ms == u64::MAX { return Err("Invalid identity timestamp sentinel".into()); }
            let time = entry.recorded_at_unix_ms.min(now_ms);
            result.host_identities.insert(host, HostIdentityEntry { revision: entry.revision, fingerprint: entry.fingerprint, recorded_at_unix_ms: Some(time), pending_since_disk: time });
        }
        let hosts_raw = object_entries(input).map_err(|e| e.to_string())?.into_iter().find(|(key, _)| key == "hosts").map(|(_, raw)| raw).unwrap_or_else(|| "{}".into());
        if !hosts_raw.trim_start().starts_with('{') { return Err("Tunnel hosts must be an object".into()); }
        for (host, raw) in object_entries(&hosts_raw).map_err(|e| e.to_string())? {
            if !raw.trim_start().starts_with('[') {
                result.invalid_rows.insert(host.clone(), vec![raw.clone()]); result.invalid_host_values.insert(host.clone(), raw);
                result.diagnostics.push(format!("Undecodable tunnel list for host {host}")); continue;
            }
            let mut valid = Vec::new();
            for row in array_rows(&raw).map_err(|e| e.to_string())? {
                let decoded = serde_json::from_str::<TunnelSpec>(&row).ok().filter(|spec| validate_saved_spec(spec).is_ok());
                if let Some(spec) = decoded {
                    let mut candidate = valid.clone(); candidate.push(spec.clone());
                    if validate_specs_for_host(&candidate).is_empty() { valid.push(spec); continue; }
                }
                result.diagnostics.push(format!("Undecodable tunnel definition for host {host}"));
                result.invalid_rows.entry(host.clone()).or_default().push(row);
            }
            result.hosts.insert(host, valid);
        }
        for (key, value) in root { if !["version", "definitions_revision", "host_revisions", "host_identities", "hosts", "revision_floor"].contains(&key.as_str()) { result.extra.insert(key.clone(), value.clone()); } }
        result.refresh_status(); Ok(result)
    };
    match decode() {
        Ok(result) => { for diagnostic in &result.diagnostics { tracing::warn!(path = %path.display(), "{diagnostic}"); } Ok(result) }
        Err(diagnostic) => { tracing::warn!(path = %path.display(), "Corrupt tunnel store: {diagnostic}"); store.store_status.kind = "corrupt".into(); store.diagnostics.push(diagnostic); Ok(store) }
    }
}

pub struct StoreLock {
    path: PathBuf,
    #[cfg(unix)]
    _file: fs::File,
}
/// The fd's lifetime is the lock lifetime; crashes never leave a stale lock.
pub fn lock_store(path: &Path) -> Result<StoreLock, IpcError> {
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(io_error)?; }
        let sidecar = path.with_file_name("ssh_tunnels.lock");
        let file = fs::OpenOptions::new().read(true).write(true).create(true).mode(0o600).open(&sidecar).map_err(|e| {
            store_error(&path.to_string_lossy(), "store_lock_unavailable").with_details(json!({"reason": "store_lock_unavailable", "path": path, "osError": e.raw_os_error()}))
        })?;
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 { break; }
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EWOULDBLOCK) | Some(libc::EINTR) => continue,
                _ => return Err(store_error(&path.to_string_lossy(), "store_lock_unavailable")),
            }
        }
        Ok(StoreLock { path: path.to_path_buf(), _file: file })
    }
    // Windows commands are gated before reaching the store. This permits pure
    // store serialization tests there without inventing a dead lock primitive.
    #[cfg(all(not(unix), test))]
    { if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(io_error)?; }
      Ok(StoreLock { path: path.to_path_buf() }) }
    #[cfg(all(not(unix), not(test)))]
    { Err(store_error(&path.to_string_lossy(), "store_lock_unavailable")) }
}
#[cfg(unix)]
impl Drop for StoreLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // Closing the descriptor releases flock, including on an unlock error.
        if unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_UN) } != 0 {
            tracing::warn!("Failed to unlock tunnel store: {}", std::io::Error::last_os_error());
        }
    }
}

/// Deterministic write failure injection for authority reconciliation tests.
#[derive(Debug)]
pub struct WriteFault;
impl StoreLock {
    pub fn load(&self, now_ms: u64) -> Result<TunnelStore, IpcError> { load_store(&self.path, now_ms) }
    /// `force` waives ONLY revision comparison. Corrupt and foreign-version
    /// files remain protected; partial rows survive unless discard_invalid is
    /// explicitly true, and then ONLY this host's unreadable rows are discarded.
    pub fn save_specs(&self, host_id: &str, specs: Vec<TunnelSpec>, expected_revision: Option<u64>, force: bool, discard_invalid: bool, memory: &TunnelStore, now_ms: u64) -> Result<TunnelStore, IpcError> {
        if expected_revision.is_none() && !force {
            return Err(IpcError::new(IpcErrorCode::InvalidArgument, "Tunnel definitions revision is required").with_details(json!({"reason": "revision_required"})));
        }
        for spec in &specs { validate_saved_spec(spec)?; }
        if !validate_specs_for_host(&specs).is_empty() { return Err(argument("specs")); }
        let mut store = self.load(now_ms)?; store.writable(false)?;
        if expected_revision.is_some_and(|revision| revision != store.definitions_revision) {
            return Err(IpcError::new(IpcErrorCode::RequestConflict, "Tunnel definitions changed").with_details(json!({"reason": "tunnel_store_conflict", "revision": store.definitions_revision})));
        }
        store.merge_authority(memory);
        // When editing a malformed non-array host value, retain that exact raw
        // value as an invalid array row beside the newly saved definitions.
        // Unrelated hosts retain even their original malformed container shape.
        if !specs.is_empty() { store.invalid_host_values.remove(host_id); }
        store.hosts.insert(host_id.into(), specs);
        if discard_invalid { store.invalid_rows.remove(host_id); store.invalid_host_values.remove(host_id); }
        store.definitions_revision = store.definitions_revision.checked_add(1).ok_or_else(|| IpcError::internal("Tunnel definitions revision exhausted"))?;
        store.refresh_status(); self.write(&store, None)?; Ok(store)
    }
    pub fn persist_authority(&self, memory: &TunnelStore, now_ms: u64) -> Result<TunnelStore, IpcError> {
        self.persist_authority_with_fault(memory, now_ms, None)
    }
    pub fn persist_authority_with_fault(&self, memory: &TunnelStore, now_ms: u64, fault: Option<&WriteFault>) -> Result<TunnelStore, IpcError> {
        let mut store = self.load(now_ms)?; store.writable(true)?; store.merge_authority(memory); self.write(&store, fault)?; Ok(store)
    }
    /// Stamp disk only if this exact revision still owns both the identity and
    /// max(file,memory) authority. Always stamp memory, including on corruption.
    pub fn stamp_identity(&self, memory: &mut TunnelStore, host_id: &str, rev: u64, now_ms: u64) -> Result<(), IpcError> {
        memory.stamp_identity(host_id, rev, now_ms);
        let mut store = self.load(now_ms)?; store.writable(true)?;
        if store.host_revision(host_id).max(memory.host_revision(host_id)) == rev
            && store.host_identities.get(host_id).is_some_and(|entry| entry.revision == rev) {
            store.merge_authority(memory); store.stamp_identity(host_id, rev, now_ms); self.write(&store, None)?;
        }
        Ok(())
    }
    /// Only the explicit reset may move a wholly corrupt file aside. A newer
    /// version is not corruption, and neither are partially undecodable rows.
    pub fn reset(&self, now_seconds: u64, memory: &TunnelStore) -> Result<PathBuf, IpcError> {
        let store = self.load(now_seconds.saturating_mul(1000))?;
        if store.store_status.kind != "corrupt" {
            return Err(IpcError::new(IpcErrorCode::InvalidArgument, "Tunnel store is not corrupt").with_details(json!({"reason": "store_not_corrupt", "path": self.path})));
        }
        let quarantine = self.path.with_file_name(format!("ssh_tunnels.corrupt-{now_seconds}.json"));
        if quarantine.exists() { return Err(IpcError::new(IpcErrorCode::RequestConflict, "Tunnel quarantine path already exists")); }
        fs::rename(&self.path, &quarantine).map_err(io_error)?;
        let mut fresh = TunnelStore::default(); fresh.store_status.path = self.path.to_string_lossy().into_owned(); fresh.merge_authority(memory); fresh.seed_floor(now_seconds);
        self.write(&fresh, None)?;
        Ok(quarantine)
    }
    fn write(&self, store: &TunnelStore, fault: Option<&WriteFault>) -> Result<(), IpcError> {
        if fault.is_some() { return Err(IpcError::new(IpcErrorCode::IoError, "Injected tunnel store write fault")); }
        let mut envelope = store.extra.clone();
        envelope.insert("version".into(), json!(1));
        envelope.insert("definitions_revision".into(), json!(store.definitions_revision));
        envelope.insert("host_revisions".into(), json!(store.host_revisions));
        envelope.insert("revision_floor".into(), json!(store.revision_floor));
        if store.host_identities.values().any(|entry| entry.recorded_at_unix_ms.unwrap_or(entry.pending_since_disk) == u64::MAX) {
            return Err(argument("recordedAtUnixMs"));
        }
        let identities: HashMap<_, _> = store.host_identities.iter().filter(|(host, entry)| entry.revision == store.host_revision(host)).map(|(host, entry)| {
            (host.clone(), DiskIdentity { revision: entry.revision, fingerprint: entry.fingerprint.clone(), recorded_at_unix_ms: entry.recorded_at_unix_ms.unwrap_or(entry.pending_since_disk) })
        }).collect();
        envelope.insert("host_identities".into(), serde_json::to_value(identities).map_err(|e| IpcError::internal(e.to_string()))?);
        let mut hosts: Vec<_> = store.hosts.keys().chain(store.invalid_rows.keys()).cloned().collect(); hosts.sort(); hosts.dedup();
        let mut host_json = Vec::new();
        for host in hosts {
            let key = serde_json::to_string(&host).map_err(|e| IpcError::internal(e.to_string()))?;
            if let Some(raw) = store.invalid_host_values.get(&host) { host_json.push(format!("{key}:{raw}")); continue; }
            let mut rows: Vec<String> = store.hosts.get(&host).into_iter().flatten().map(serde_json::to_string).collect::<Result<_, _>>().map_err(|e| IpcError::internal(e.to_string()))?;
            rows.extend(store.invalid_rows.get(&host).into_iter().flatten().cloned());
            host_json.push(format!("{key}:[{}]", rows.join(",")));
        }
        let mut fields: Vec<String> = envelope.into_iter().map(|(key, value)| format!("{}:{value}", serde_json::to_string(&key).expect("string key serializes"))).collect(); fields.sort();
        fields.push(format!("\"hosts\":{{{}}}", host_json.join(",")));
        let serialized = format!("{{{}}}\n", fields.join(","));
        let tmp = self.path.with_extension(format!("json.tmp.{}.{}", std::process::id(), uuid::Uuid::new_v4()));
        let result = fs::write(&tmp, serialized.as_bytes()).and_then(|()| fs::rename(&tmp, &self.path));
        if result.is_err() {
            if let Err(error) = fs::remove_file(&tmp) { if error.kind() != std::io::ErrorKind::NotFound { tracing::warn!("Failed to remove tunnel store temporary file: {error}"); } }
        }
        result.map_err(io_error)
    }
}

#[cfg(test)]
#[path = "tunnel_store_tests.rs"]
mod tests;
