//! Session-host registry files (design rev3 sections 2, 3 and 4).
//!
//! Everything lives in dataDir/session-hosts. Files are written by temp file, flush, then
//! rename. Liveness is a pure classifier over probe results the caller collects, so the
//! retention rule (remove only on confirmed death) is testable on every platform.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::protocol::{encode_hex, Epoch, RejectCode, Secret32, HOST_PROTOCOL_VERSION};

pub const SESSION_HOSTS_DIR: &str = "session-hosts";
pub const CONTROLLER_EPOCH_FILE: &str = "controller-epoch";
const MAX_SESSION_ID_LEN: usize = 128;
const MAX_REGISTRY_FILE_BYTES: u64 = 1 << 20;
const PIPE_PREFIX: &str = r"\\.\pipe\ferryx-sh-";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("{op} {path:?}: {message}")]
    Io {
        op: &'static str,
        path: PathBuf,
        kind: io::ErrorKind,
        message: String,
    },
    #[error("{path:?}: invalid json: {message}")]
    Json { path: PathBuf, message: String },
    #[error("{path:?}: larger than the 1 MiB registry file cap")]
    TooLarge { path: PathBuf },
    #[error("session id {0:?} is not a safe file name")]
    InvalidSessionId(String),
    #[error("{path:?}: record is for session {found:?}, expected {expected:?}")]
    SessionIdMismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },
    #[error("{path:?}: host protocol {found} is not supported")]
    UnsupportedHostProtocol { path: PathBuf, found: u32 },
    #[error("{path:?}: controller epoch {text:?} is not a u64")]
    CorruptEpoch { path: PathBuf, text: String },
    #[error("controller epoch overflowed")]
    EpochOverflow,
}

impl RegistryError {
    fn io(op: &'static str, path: &Path, error: &io::Error) -> Self {
        Self::Io {
            op,
            path: path.to_path_buf(),
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

/// Session ids become file and pipe names, so only [A-Za-z0-9_-] is accepted.
pub fn validate_session_id(session_id: &str) -> Result<(), RegistryError> {
    let safe = !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_LEN
        && session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if safe {
        Ok(())
    } else {
        Err(RegistryError::InvalidSessionId(session_id.to_string()))
    }
}

/// Pipe name for a session: \\.\pipe\ferryx-sh-SESSIONID-32HEX.
pub fn pipe_name(session_id: &str, suffix: &[u8; 16]) -> Result<String, RegistryError> {
    validate_session_id(session_id)?;
    Ok(format!("{PIPE_PREFIX}{session_id}-{}", encode_hex(suffix)))
}

pub fn session_hosts_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(SESSION_HOSTS_DIR)
}

pub fn record_path(dir: &Path, session_id: &str) -> Result<PathBuf, RegistryError> {
    validate_session_id(session_id)?;
    Ok(dir.join(format!("{session_id}.json")))
}

pub fn spec_path(dir: &Path, session_id: &str) -> Result<PathBuf, RegistryError> {
    validate_session_id(session_id)?;
    Ok(dir.join(format!("{session_id}.spec.json")))
}

/// handover.ES.json. The '.' is outside the session id alphabet, so no record path
/// (SESSIONID.json) can ever name a manifest.
pub fn manifest_path(dir: &Path, epoch: Epoch) -> PathBuf {
    dir.join(format!("handover.{}.json", epoch.0))
}

/// Written by the daemon, read then deleted by the host at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSpec {
    pub session_id: String,
    pub pipe_name: String,
    pub token: Secret32,
    pub cwd: PathBuf,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

/// Written by the host after the shell starts; the controller verifies identity on reconnect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRecord {
    pub host_protocol: u32,
    pub session_id: String,
    pub host_pid: u32,
    pub host_creation_time: u64,
    pub exe_path: PathBuf,
    pub pipe_name: String,
    pub token: Secret32,
    pub shell_pid: u32,
    pub created_epoch: Epoch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSession {
    pub session_id: String,
    pub grant_nonce: Secret32,
}

/// handover.ES.json, written by the predecessor after every host authorized the transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverManifest {
    pub es: Epoch,
    pub predecessor_pid: u32,
    pub predecessor_creation_time: u64,
    pub sessions: Vec<ManifestSession>,
}

impl HandoverManifest {
    pub fn nonce_for(&self, session_id: &str) -> Option<&Secret32> {
        self.sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .map(|s| &s.grant_nonce)
    }
}

pub fn write_spec(dir: &Path, spec: &HostSpec) -> Result<PathBuf, RegistryError> {
    let path = spec_path(dir, &spec.session_id)?;
    write_json_atomic(&path, spec)?;
    Ok(path)
}

/// Host startup step 1: read the spec, then delete it so the token is not left on disk.
pub fn take_spec(path: &Path) -> Result<HostSpec, RegistryError> {
    let spec: HostSpec = read_json(path)?.ok_or_else(|| {
        RegistryError::io("read", path, &io::Error::from(io::ErrorKind::NotFound))
    })?;
    fs::remove_file(path).map_err(|e| RegistryError::io("remove", path, &e))?;
    validate_session_id(&spec.session_id)?;
    Ok(spec)
}

pub fn write_record(dir: &Path, record: &HostRecord) -> Result<PathBuf, RegistryError> {
    let path = record_path(dir, &record.session_id)?;
    write_json_atomic(&path, record)?;
    Ok(path)
}

pub fn read_record(dir: &Path, session_id: &str) -> Result<Option<HostRecord>, RegistryError> {
    let path = record_path(dir, session_id)?;
    let Some(record) = read_json::<HostRecord>(&path)? else {
        return Ok(None);
    };
    check_record(&path, session_id, record).map(Some)
}

/// Every record file in the directory with its parse result. Unreadable records are returned
/// as errors, never skipped, so recovery keeps them for the next boot.
pub fn list_records(
    dir: &Path,
) -> Result<Vec<(String, Result<HostRecord, RegistryError>)>, RegistryError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(RegistryError::io("read_dir", dir, &e)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| RegistryError::io("read_dir", dir, &e))?;
        let name = entry.file_name();
        let Some(session_id) = name.to_str().and_then(record_session_id) else {
            continue;
        };
        let result = read_record(dir, session_id).and_then(|found| {
            found.ok_or_else(|| {
                RegistryError::io(
                    "read",
                    &entry.path(),
                    &io::Error::from(io::ErrorKind::NotFound),
                )
            })
        });
        out.push((session_id.to_string(), result));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn record_session_id(file_name: &str) -> Option<&str> {
    let stem = file_name.strip_suffix(".json")?;
    if stem.ends_with(".spec") || validate_session_id(stem).is_err() {
        return None;
    }
    Some(stem)
}

fn check_record(
    path: &Path,
    session_id: &str,
    record: HostRecord,
) -> Result<HostRecord, RegistryError> {
    if record.session_id != session_id {
        return Err(RegistryError::SessionIdMismatch {
            path: path.to_path_buf(),
            expected: session_id.to_string(),
            found: record.session_id,
        });
    }
    if record.host_protocol != HOST_PROTOCOL_VERSION {
        return Err(RegistryError::UnsupportedHostProtocol {
            path: path.to_path_buf(),
            found: record.host_protocol,
        });
    }
    Ok(record)
}

/// The host's own clean-exit removal (design section 5). Missing files are not an error.
pub fn remove_own_record(dir: &Path, session_id: &str) -> Result<(), RegistryError> {
    remove_if_present(&record_path(dir, session_id)?)
}

/// The lock holder's removal: only a confirmed Dead liveness removes a record. Returns whether
/// the record was removed.
pub fn remove_record_if_dead(
    dir: &Path,
    session_id: &str,
    liveness: &Liveness,
) -> Result<bool, RegistryError> {
    if !liveness.permits_removal() {
        return Ok(false);
    }
    remove_if_present(&record_path(dir, session_id)?)?;
    Ok(true)
}

pub fn write_manifest(dir: &Path, manifest: &HandoverManifest) -> Result<PathBuf, RegistryError> {
    for session in &manifest.sessions {
        validate_session_id(&session.session_id)?;
    }
    let path = manifest_path(dir, manifest.es);
    write_json_atomic(&path, manifest)?;
    Ok(path)
}

pub fn read_manifest(path: &Path) -> Result<Option<HandoverManifest>, RegistryError> {
    read_json(path)
}

pub fn remove_manifest(path: &Path) -> Result<(), RegistryError> {
    remove_if_present(path)
}

pub fn read_controller_epoch(dir: &Path) -> Result<Epoch, RegistryError> {
    let path = dir.join(CONTROLLER_EPOCH_FILE);
    let Some(bytes) = read_capped(&path)? else {
        return Ok(Epoch::ZERO);
    };
    let text = String::from_utf8_lossy(&bytes).trim().to_string();
    text.parse::<u64>()
        .map(Epoch)
        .map_err(|_| RegistryError::CorruptEpoch { path, text })
}

/// Bumps and persists the controller epoch. Call only while holding daemon.lock; the result is
/// never reused. A corrupt file is an error rather than a reset, because a reset could reuse
/// an epoch a host already burned.
pub fn controller_epoch_bump(dir: &Path) -> Result<Epoch, RegistryError> {
    let next = read_controller_epoch(dir)?
        .checked_next()
        .ok_or(RegistryError::EpochOverflow)?;
    write_atomic(
        &dir.join(CONTROLLER_EPOCH_FILE),
        next.0.to_string().as_bytes(),
    )?;
    Ok(next)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), RegistryError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| RegistryError::Json {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    write_atomic(path, &bytes)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, RegistryError> {
    let Some(bytes) = read_capped(path)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| RegistryError::Json {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
}

fn read_capped(path: &Path) -> Result<Option<Vec<u8>>, RegistryError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(RegistryError::io("open", path, &e)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_REGISTRY_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| RegistryError::io("read", path, &e))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REGISTRY_FILE_BYTES {
        return Err(RegistryError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    Ok(Some(bytes))
}

fn remove_if_present(path: &Path) -> Result<(), RegistryError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(RegistryError::io("remove", path, &e)),
    }
}

/// Temp file in the same directory, write, flush to disk, rename over the target. rename
/// replaces an existing destination on Unix and Windows alike.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    let parent = path.parent().ok_or_else(|| {
        RegistryError::io("write", path, &io::Error::from(io::ErrorKind::InvalidInput))
    })?;
    fs::create_dir_all(parent).map_err(|e| RegistryError::io("create_dir_all", parent, &e))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("registry");
    let tmp = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        encode_hex(&rand::random::<[u8; 8]>())
    ));
    let result = write_and_rename(&tmp, path, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    sync_parent_dir(parent);
    Ok(())
}

fn write_and_rename(tmp: &Path, path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(tmp)
        .map_err(|e| RegistryError::io("create", tmp, &e))?;
    file.write_all(bytes)
        .map_err(|e| RegistryError::io("write", tmp, &e))?;
    file.sync_all()
        .map_err(|e| RegistryError::io("sync", tmp, &e))?;
    drop(file);
    fs::rename(tmp, path).map_err(|e| RegistryError::io("rename", path, &e))
}

/// Best effort: makes the rename durable on Unix. Windows has no directory handle flush here.
fn sync_parent_dir(parent: &Path) {
    #[cfg(unix)]
    {
        if let Err(error) = File::open(parent).and_then(|dir| dir.sync_all()) {
            tracing::warn!(path = %parent.display(), %error, "session-host registry directory fsync failed");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
    }
}

/// Result of OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, hostPid) and the
/// follow-up queries on that handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessProbe {
    Opened {
        /// WaitForSingleObject(h, 0) == WAIT_OBJECT_0.
        exited: bool,
        /// GetProcessTimes creation time, or the OS error code when the query failed.
        creation_time: Result<u64, i32>,
    },
    /// OpenProcess failed with ERROR_INVALID_PARAMETER: no process has that pid.
    NoSuchProcess,
    AccessDenied,
    /// Any other OpenProcess error code.
    OpenFailed(i32),
}

/// Result of opening or talking to the host pipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipeProbe {
    /// CreateFile on the pipe failed with ERROR_FILE_NOT_FOUND.
    NotFound,
    Connected,
    /// ERROR_PIPE_BUSY.
    Busy,
    TimedOut,
    /// The host answered Hello with Rejected.
    Rejected(RejectCode),
    /// Any other open or read error code.
    Failed(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadReason {
    Exited,
    IdentityMismatch,
    NoProcessNoPipe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownReason {
    AccessDenied,
    OpenFailed(i32),
    CreationTimeFailed(i32),
    PidMissingPipe(PipeProbe),
    PidMissingPipeNotProbed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Dead(DeadReason),
    Unknown(UnknownReason),
}

impl Liveness {
    /// Only positive confirmation of death allows removing a record or its state entry.
    pub fn permits_removal(&self) -> bool {
        matches!(self, Self::Dead(_))
    }
}

/// host_liveness over injected probes. pipe is consulted only when the pid is missing; pass
/// None when it was not probed. Anything short of positive confirmation is Unknown.
pub fn classify_liveness(
    record: &HostRecord,
    process: &ProcessProbe,
    pipe: Option<&PipeProbe>,
) -> Liveness {
    match process {
        ProcessProbe::Opened {
            creation_time: Err(code),
            ..
        } => Liveness::Unknown(UnknownReason::CreationTimeFailed(*code)),
        ProcessProbe::Opened {
            creation_time: Ok(time),
            ..
        } if *time != record.host_creation_time => Liveness::Dead(DeadReason::IdentityMismatch),
        ProcessProbe::Opened { exited: true, .. } => Liveness::Dead(DeadReason::Exited),
        ProcessProbe::Opened { exited: false, .. } => Liveness::Alive,
        ProcessProbe::NoSuchProcess => match pipe {
            Some(PipeProbe::NotFound) => Liveness::Dead(DeadReason::NoProcessNoPipe),
            Some(other) => Liveness::Unknown(UnknownReason::PidMissingPipe(other.clone())),
            None => Liveness::Unknown(UnknownReason::PidMissingPipeNotProbed),
        },
        ProcessProbe::AccessDenied => Liveness::Unknown(UnknownReason::AccessDenied),
        ProcessProbe::OpenFailed(code) => Liveness::Unknown(UnknownReason::OpenFailed(*code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session_id: &str) -> HostRecord {
        HostRecord {
            host_protocol: HOST_PROTOCOL_VERSION,
            session_id: session_id.to_string(),
            host_pid: 4321,
            host_creation_time: 133_000_000_000,
            exe_path: PathBuf::from(
                r"C:\Users\u\AppData\Local\Ferryx\session-host\v-0123456789abcdef\ferryx.exe",
            ),
            pipe_name: pipe_name(session_id, &[0xab; 16]).unwrap(),
            token: Secret32::from_bytes([3; 32]),
            shell_pid: 9876,
            created_epoch: Epoch(5),
        }
    }

    fn spec(session_id: &str) -> HostSpec {
        HostSpec {
            session_id: session_id.to_string(),
            pipe_name: pipe_name(session_id, &[1; 16]).unwrap(),
            token: Secret32::from_bytes([4; 32]),
            cwd: PathBuf::from("C:/work"),
            program: "pwsh.exe".into(),
            args: vec!["-NoLogo".into()],
            env: vec![("FERRYX_SESSION_ID".into(), session_id.into())],
            cols: 120,
            rows: 40,
        }
    }

    fn opened(exited: bool, creation_time: Result<u64, i32>) -> ProcessProbe {
        ProcessProbe::Opened {
            exited,
            creation_time,
        }
    }

    #[test]
    fn pipe_name_has_the_documented_shape() {
        let name = pipe_name("abc-123", &[0x0f; 16]).unwrap();
        assert_eq!(
            name,
            format!(r"\\.\pipe\ferryx-sh-abc-123-{}", "0f".repeat(16))
        );
    }

    #[test]
    fn unsafe_session_ids_are_rejected_before_touching_paths() {
        let too_long = "a".repeat(129);
        for bad in ["", "..", "a/b", r"a\b", "a.json", "x y", too_long.as_str()] {
            assert!(
                matches!(
                    record_path(Path::new("d"), bad),
                    Err(RegistryError::InvalidSessionId(_))
                ),
                "{bad:?}"
            );
        }
        assert!(record_path(Path::new("d"), "Ab_9-z").is_ok());
    }

    #[test]
    fn record_round_trips_with_camel_case_keys_and_hex_token() {
        let dir = tempfile::tempdir().unwrap();
        let written = record("s1");
        let path = write_record(dir.path(), &written).unwrap();
        assert_eq!(path, dir.path().join("s1.json"));
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["hostCreationTime"], 133_000_000_000u64);
        assert_eq!(value["createdEpoch"], 5);
        assert_eq!(value["token"], "03".repeat(32));
        assert_eq!(read_record(dir.path(), "s1").unwrap(), Some(written));
        assert_eq!(read_record(dir.path(), "missing").unwrap(), None);
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), &record("s1")).unwrap();
        let mut second = record("s1");
        second.shell_pid = 1;
        write_record(dir.path(), &second).unwrap();
        assert_eq!(read_record(dir.path(), "s1").unwrap().unwrap().shell_pid, 1);
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["s1.json".to_string()]);
    }

    #[test]
    fn read_record_rejects_mismatched_session_and_protocol() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("s1.json"),
            serde_json::to_vec(&record("s2")).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            read_record(dir.path(), "s1"),
            Err(RegistryError::SessionIdMismatch { .. })
        ));
        let mut future = record("s3");
        future.host_protocol = 2;
        fs::write(
            dir.path().join("s3.json"),
            serde_json::to_vec(&future).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            read_record(dir.path(), "s3"),
            Err(RegistryError::UnsupportedHostProtocol { found: 2, .. })
        ));
    }

    #[test]
    fn oversized_record_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let too_big = vec![b' '; usize::try_from(MAX_REGISTRY_FILE_BYTES).unwrap() + 1];
        fs::write(dir.path().join("s1.json"), too_big).unwrap();
        assert!(matches!(
            read_record(dir.path(), "s1"),
            Err(RegistryError::TooLarge { .. })
        ));
    }

    #[test]
    fn list_records_keeps_unreadable_records_and_skips_other_files() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), &record("good")).unwrap();
        fs::write(dir.path().join("broken.json"), b"{").unwrap();
        write_spec(dir.path(), &spec("pending")).unwrap();
        write_manifest(
            dir.path(),
            &HandoverManifest {
                es: Epoch(3),
                predecessor_pid: 1,
                predecessor_creation_time: 2,
                sessions: vec![],
            },
        )
        .unwrap();
        fs::write(dir.path().join(CONTROLLER_EPOCH_FILE), b"3").unwrap();
        let listed = list_records(dir.path()).unwrap();
        let ids: Vec<&str> = listed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["broken", "good"]);
        assert!(matches!(listed[0].1, Err(RegistryError::Json { .. })));
        assert_eq!(listed[1].1.as_ref().unwrap(), &record("good"));
        assert!(list_records(&dir.path().join("absent")).unwrap().is_empty());
    }

    #[test]
    fn take_spec_reads_then_deletes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_spec(dir.path(), &spec("s1")).unwrap();
        assert_eq!(path, dir.path().join("s1.spec.json"));
        assert_eq!(take_spec(&path).unwrap(), spec("s1"));
        assert!(!path.exists());
        assert!(matches!(
            take_spec(&path),
            Err(RegistryError::Io {
                kind: io::ErrorKind::NotFound,
                ..
            })
        ));
    }

    #[test]
    fn manifest_round_trips_with_per_session_nonces() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = HandoverManifest {
            es: Epoch(12),
            predecessor_pid: 77,
            predecessor_creation_time: 88,
            sessions: vec![
                ManifestSession {
                    session_id: "a".into(),
                    grant_nonce: Secret32::from_bytes([1; 32]),
                },
                ManifestSession {
                    session_id: "b".into(),
                    grant_nonce: Secret32::from_bytes([2; 32]),
                },
            ],
        };
        let path = write_manifest(dir.path(), &manifest).unwrap();
        assert_eq!(path, dir.path().join("handover.12.json"));
        let read = read_manifest(&path).unwrap().unwrap();
        assert_eq!(read, manifest);
        assert_eq!(read.nonce_for("b"), Some(&Secret32::from_bytes([2; 32])));
        assert_eq!(read.nonce_for("c"), None);
        remove_manifest(&path).unwrap();
        assert_eq!(read_manifest(&path).unwrap(), None);
    }

    #[test]
    fn manifest_with_unsafe_session_id_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = HandoverManifest {
            es: Epoch(1),
            predecessor_pid: 1,
            predecessor_creation_time: 1,
            sessions: vec![ManifestSession {
                session_id: "../x".into(),
                grant_nonce: Secret32::from_bytes([1; 32]),
            }],
        };
        assert!(matches!(
            write_manifest(dir.path(), &manifest),
            Err(RegistryError::InvalidSessionId(_))
        ));
        assert!(!manifest_path(dir.path(), Epoch(1)).exists());
    }

    #[test]
    fn controller_epoch_bump_is_monotonic_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_controller_epoch(dir.path()).unwrap(), Epoch::ZERO);
        assert_eq!(controller_epoch_bump(dir.path()).unwrap(), Epoch(1));
        assert_eq!(controller_epoch_bump(dir.path()).unwrap(), Epoch(2));
        assert_eq!(read_controller_epoch(dir.path()).unwrap(), Epoch(2));
        assert_eq!(
            fs::read_to_string(dir.path().join(CONTROLLER_EPOCH_FILE)).unwrap(),
            "2"
        );
    }

    #[test]
    fn corrupt_or_exhausted_controller_epoch_is_an_error_not_a_reset() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CONTROLLER_EPOCH_FILE), b"nope").unwrap();
        assert!(matches!(
            controller_epoch_bump(dir.path()),
            Err(RegistryError::CorruptEpoch { .. })
        ));
        fs::write(dir.path().join(CONTROLLER_EPOCH_FILE), u64::MAX.to_string()).unwrap();
        assert_eq!(
            controller_epoch_bump(dir.path()),
            Err(RegistryError::EpochOverflow)
        );
        assert_eq!(read_controller_epoch(dir.path()).unwrap(), Epoch(u64::MAX));
    }

    #[test]
    fn liveness_is_dead_only_on_positive_confirmation() {
        let rec = record("s1");
        let ct = rec.host_creation_time;
        assert_eq!(
            classify_liveness(&rec, &opened(false, Ok(ct)), None),
            Liveness::Alive
        );
        assert_eq!(
            classify_liveness(&rec, &opened(true, Ok(ct)), None),
            Liveness::Dead(DeadReason::Exited)
        );
        assert_eq!(
            classify_liveness(&rec, &opened(false, Ok(ct + 1)), None),
            Liveness::Dead(DeadReason::IdentityMismatch)
        );
        assert_eq!(
            classify_liveness(
                &rec,
                &ProcessProbe::NoSuchProcess,
                Some(&PipeProbe::NotFound)
            ),
            Liveness::Dead(DeadReason::NoProcessNoPipe)
        );
    }

    #[test]
    fn liveness_is_unknown_for_every_ambiguous_probe() {
        let rec = record("s1");
        let unknown = [
            classify_liveness(
                &rec,
                &ProcessProbe::AccessDenied,
                Some(&PipeProbe::NotFound),
            ),
            classify_liveness(&rec, &ProcessProbe::OpenFailed(1450), None),
            classify_liveness(&rec, &opened(true, Err(5)), None),
            classify_liveness(&rec, &ProcessProbe::NoSuchProcess, None),
            classify_liveness(&rec, &ProcessProbe::NoSuchProcess, Some(&PipeProbe::Busy)),
            classify_liveness(
                &rec,
                &ProcessProbe::NoSuchProcess,
                Some(&PipeProbe::TimedOut),
            ),
            classify_liveness(
                &rec,
                &ProcessProbe::NoSuchProcess,
                Some(&PipeProbe::Failed(109)),
            ),
            classify_liveness(
                &rec,
                &ProcessProbe::NoSuchProcess,
                Some(&PipeProbe::Connected),
            ),
            classify_liveness(
                &rec,
                &ProcessProbe::NoSuchProcess,
                Some(&PipeProbe::Rejected(RejectCode::Busy)),
            ),
            classify_liveness(
                &rec,
                &ProcessProbe::NoSuchProcess,
                Some(&PipeProbe::Rejected(RejectCode::NoGrant)),
            ),
        ];
        for liveness in unknown {
            assert!(matches!(liveness, Liveness::Unknown(_)), "{liveness:?}");
            assert!(!liveness.permits_removal());
        }
    }

    #[test]
    fn remove_record_if_dead_keeps_alive_and_unknown_records() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), &record("s1")).unwrap();
        assert!(!remove_record_if_dead(dir.path(), "s1", &Liveness::Alive).unwrap());
        assert!(!remove_record_if_dead(
            dir.path(),
            "s1",
            &Liveness::Unknown(UnknownReason::AccessDenied)
        )
        .unwrap());
        assert!(read_record(dir.path(), "s1").unwrap().is_some());
        assert!(
            remove_record_if_dead(dir.path(), "s1", &Liveness::Dead(DeadReason::Exited)).unwrap()
        );
        assert_eq!(read_record(dir.path(), "s1").unwrap(), None);
        remove_own_record(dir.path(), "s1").unwrap();
    }

    #[test]
    fn session_named_like_a_manifest_neither_collides_nor_disappears() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = HandoverManifest {
            es: Epoch(12),
            predecessor_pid: 77,
            predecessor_creation_time: 88,
            sessions: vec![ManifestSession {
                session_id: "handover-12".into(),
                grant_nonce: Secret32::from_bytes([1; 32]),
            }],
        };
        let manifest_file = write_manifest(dir.path(), &manifest).unwrap();
        let record_file = write_record(dir.path(), &record("handover-12")).unwrap();
        assert_ne!(manifest_file, record_file);
        assert_eq!(read_manifest(&manifest_file).unwrap(), Some(manifest));
        assert_eq!(
            read_record(dir.path(), "handover-12").unwrap(),
            Some(record("handover-12"))
        );
        let listed = list_records(dir.path()).unwrap();
        let ids: Vec<&str> = listed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["handover-12"]);
        assert_eq!(listed[0].1.as_ref().unwrap(), &record("handover-12"));
    }
}
