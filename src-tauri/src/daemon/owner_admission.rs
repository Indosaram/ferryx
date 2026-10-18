//! Non-destructive daemon owner admission.
//!
//! Every start site asks classify_owner before it starts a daemon. Only an
//! Absent owner may be started; an Alive or Unknown owner is never reloaded,
//! replaced, or killed.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::client::DAEMON_READY_TIMEOUT;

/// Classification of the current daemon owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerState {
    Alive { pid: u32, start_time: u64 },
    Unknown { reason: String },
    Absent,
}

/// Combined state of the persistent and the legacy daemon lock.
/// Held if either lock is held, Free if both are free, Error otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockState {
    Held,
    Free,
    Error,
}

/// What the daemon endpoint does when a client connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Endpoint {
    /// Any frame came back, including a protocol mismatch or DAEMON_RETIRED.
    Answers,
    /// The connection succeeded but no frame arrived within the probe window.
    Silent,
    /// Connection refused, or no socket / port file exists.
    Missing,
}

/// <runtime_dir>/owner.json, written by the daemon after it holds its locks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OwnerRecord {
    pub pid: u32,
    pub start_time: u64,
    pub generation_key: String,
    pub epoch: u64,
}

/// <runtime_dir>/starting.json, written by a launcher before it issues a start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartingRecord {
    pub launcher_pid: u32,
    pub launcher_start_time: u64,
    pub generation_key: String,
    pub started_at_unix_ms: u64,
}

/// Signals used by classify_owner. Injected so tests need no real daemon.
pub(crate) trait OwnerProbe {
    fn lock_state(&self) -> LockState;
    /// Ok(None) when owner.json does not exist; Err when it cannot be read or parsed.
    fn read_record(&self) -> Result<Option<OwnerRecord>, String>;
    /// Start time of a live process, or None when the process is dead or its
    /// start time cannot be read.
    fn process_start_time(&self, pid: u32) -> Option<u64>;
    fn endpoint(&self) -> Result<Endpoint, String>;
    /// Ok(None) when starting.json does not exist; Err when it cannot be read or parsed.
    fn read_starting(&self) -> Result<Option<StartingRecord>, String>;
    fn now_unix_ms(&self) -> u64;
}

/// A starting.json younger than this means a start is still pending.
pub(crate) fn starting_window_ms() -> u64 {
    (DAEMON_READY_TIMEOUT.as_millis() as u64).saturating_mul(2)
}

fn record_owner_alive(probe: &dyn OwnerProbe, record: &OwnerRecord) -> bool {
    probe.process_start_time(record.pid) == Some(record.start_time)
}

/// Classify the daemon owner. First matching row wins:
///
/// 1. lock Error or any probe error -> Unknown
/// 2. Held + record pid alive with equal start time -> Alive
/// 3. Held otherwise -> Unknown
/// 4. Free + endpoint Answers or Silent -> Unknown
/// 5. Free + Missing + record pid alive with equal start time -> Unknown
/// 5b. Free + Missing + starting.json younger than 2 x DAEMON_READY_TIMEOUT -> Unknown
/// 6. Free + Missing otherwise (no record, dead pid, different start time) -> Absent
pub(crate) fn classify_owner(probe: &dyn OwnerProbe) -> OwnerState {
    // Row 1.
    let lock = probe.lock_state();
    if lock == LockState::Error {
        return OwnerState::Unknown {
            reason: "daemon lock state could not be determined".to_string(),
        };
    }
    let record = match probe.read_record() {
        Ok(record) => record,
        Err(error) => {
            return OwnerState::Unknown {
                reason: format!("owner record unreadable: {error}"),
            }
        }
    };
    let endpoint = match probe.endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => {
            return OwnerState::Unknown {
                reason: format!("endpoint probe failed: {error}"),
            }
        }
    };
    let starting = match probe.read_starting() {
        Ok(starting) => starting,
        Err(error) => {
            return OwnerState::Unknown {
                reason: format!("starting record unreadable: {error}"),
            }
        }
    };

    let alive_record = record
        .as_ref()
        .filter(|record| record_owner_alive(probe, record));

    if lock == LockState::Held {
        // Row 2.
        if let Some(record) = alive_record {
            return OwnerState::Alive {
                pid: record.pid,
                start_time: record.start_time,
            };
        }
        // Row 3.
        return OwnerState::Unknown {
            reason: "daemon lock is held but its holder cannot be identified".to_string(),
        };
    }

    // Row 4.
    if endpoint != Endpoint::Missing {
        return OwnerState::Unknown {
            reason: format!("daemon lock is free but the endpoint is {endpoint:?}"),
        };
    }
    // Row 5.
    if let Some(record) = alive_record {
        return OwnerState::Unknown {
            reason: format!(
                "recorded owner pid {} is alive without lock or endpoint",
                record.pid
            ),
        };
    }
    // Row 5b.
    if let Some(starting) = starting {
        let age_ms = probe
            .now_unix_ms()
            .saturating_sub(starting.started_at_unix_ms);
        if age_ms < starting_window_ms() {
            return OwnerState::Unknown {
                reason: format!(
                    "a start by launcher pid {} is pending ({age_ms} ms old)",
                    starting.launcher_pid
                ),
            };
        }
    }
    // Row 6.
    OwnerState::Absent
}

/// Files that admission reads and writes, all under the daemon runtime directory except the
/// persistent lock. Kept as a value so tests can point it at a temporary directory.
#[derive(Debug, Clone)]
pub(crate) struct AdmissionPaths {
    pub persistent_lock: Option<PathBuf>,
    pub legacy_lock: PathBuf,
    pub endpoint: PathBuf,
    pub owner: PathBuf,
    pub starting: PathBuf,
    pub admission_lock: PathBuf,
}

impl AdmissionPaths {
    pub(crate) fn for_runtime() -> Self {
        let runtime_dir = crate::daemon::server::get_runtime_dir();
        Self {
            persistent_lock: crate::daemon::server::get_persistent_lock_path(),
            legacy_lock: crate::daemon::server::get_lock_path(),
            endpoint: crate::daemon::server::get_socket_path(),
            owner: runtime_dir.join(OWNER_FILE),
            starting: runtime_dir.join(STARTING_FILE),
            admission_lock: runtime_dir.join(ADMISSION_LOCK_FILE),
        }
    }
}

pub(crate) const OWNER_FILE: &str = "owner.json";
pub(crate) const STARTING_FILE: &str = "starting.json";
pub(crate) const ADMISSION_LOCK_FILE: &str = "admission.lock";

/// How long a caller waits for admission.lock before reporting the daemon as busy.
pub(crate) const ADMISSION_LOCK_DEADLINE: Duration = Duration::from_secs(10);
/// How long the endpoint probe waits for a frame after a successful connect.
const ENDPOINT_FRAME_WAIT: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Identifies the build generation that wrote a record. Provisional: the package version, until
/// the generation identity of todo 3 lands.
pub(crate) fn current_generation_key() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Writes a JSON record atomically: temp file, fsync, rename, then fsync of the directory on
/// Unix. The file is created with mode 0600 on Unix.
pub(crate) fn write_record_atomic<T: Serialize>(path: &Path, record: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Record path has no parent: {}", path.display()))?;
    let bytes = serde_json::to_vec(record)
        .map_err(|error| format!("Failed to encode {}: {error}", path.display()))?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "record".to_string());
    let temp = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&temp).and_then(|mut file| {
        file.write_all(&bytes)?;
        file.sync_all()
    });
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(format!("Failed to write {}: {error}", temp.display()));
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("Failed to publish {}: {error}", path.display()));
    }
    #[cfg(unix)]
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// Reads a JSON record. A missing file is Ok(None); an unreadable or malformed file is Err.
pub(crate) fn read_record<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| format!("Failed to parse {}: {error}", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Failed to read {}: {error}", path.display())),
    }
}

/// Called by the daemon right after it holds its locks: publishes owner.json and clears the
/// launcher's starting.json. owner.json is never deleted; a stale one is judged by identity.
pub(crate) fn publish_owner(paths: &AdmissionPaths, generation_key: &str, epoch: u64) -> Result<(), String> {
    let pid = std::process::id();
    let start_time = process_start_time(pid)
        .ok_or_else(|| format!("Cannot read the start time of daemon pid {pid}"))?;
    write_record_atomic(
        &paths.owner,
        &OwnerRecord {
            pid,
            start_time,
            generation_key: generation_key.to_string(),
            epoch,
        },
    )?;
    match fs::remove_file(&paths.starting) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove {}: {error}", paths.starting.display())),
    }
}

/// Start time of a live process in a platform-specific monotonic unit, or None when the
/// process is gone or the value cannot be read. Only equality between two readings matters.
#[cfg(target_os = "macos")]
pub(crate) fn process_start_time(pid: u32) -> Option<u64> {
    // SAFETY: proc_bsdinfo is a plain C struct; the all-zero pattern is valid.
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: `info` is a valid, writable proc_bsdinfo of exactly `size` bytes.
    let read = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if read != size || info.pbi_pid != pid {
        return None;
    }
    info.pbi_start_tvsec
        .checked_mul(1_000_000)?
        .checked_add(info.pbi_start_tvusec)
}

#[cfg(target_os = "linux")]
pub(crate) fn process_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name may contain spaces and parentheses; fields resume after the last ')'.
    let after_name = &stat[stat.rfind(')')? + 1..];
    // after_name starts at field 3 (state); starttime is field 22.
    after_name.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(windows)]
pub(crate) fn process_start_time(pid: u32) -> Option<u64> {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetProcessTimes(
            handle: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const STILL_ACTIVE: u32 = 259;

    // SAFETY: OpenProcess takes plain integers and returns a handle or null.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    let mut code = 0u32;
    // SAFETY: `handle` was opened above with PROCESS_QUERY_LIMITED_INFORMATION, every out
    // pointer is a valid stack value, and the handle is closed exactly once after its last use.
    let (times_ok, alive) = unsafe {
        let times_ok =
            GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
        let alive = GetExitCodeProcess(handle, &mut code) != 0 && code == STILL_ACTIVE;
        CloseHandle(handle);
        (times_ok, alive)
    };
    (times_ok && alive).then(|| (u64::from(creation.high) << 32) | u64::from(creation.low))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(crate) fn process_start_time(_pid: u32) -> Option<u64> {
    None
}

/// Probes one daemon lock without creating it. Free locks are released immediately.
fn probe_lock(path: &Path) -> Result<bool, String> {
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Failed to open {}: {error}", path.display())),
    };
    // DaemonLockFile::try_lock only fails when another handle holds the lock.
    Ok(crate::daemon::server::DaemonLockFile::try_lock(file).is_err())
}

/// The real probe over the runtime files.
pub(crate) struct RuntimeProbe<'a> {
    pub paths: &'a AdmissionPaths,
}

impl OwnerProbe for RuntimeProbe<'_> {
    fn lock_state(&self) -> LockState {
        let persistent = match &self.paths.persistent_lock {
            Some(path) => probe_lock(path),
            None => Ok(false),
        };
        let legacy = probe_lock(&self.paths.legacy_lock);
        match (persistent, legacy) {
            (Ok(true), _) | (_, Ok(true)) => LockState::Held,
            (Ok(false), Ok(false)) => LockState::Free,
            _ => LockState::Error,
        }
    }

    fn read_record(&self) -> Result<Option<OwnerRecord>, String> {
        read_record(&self.paths.owner)
    }

    fn process_start_time(&self, pid: u32) -> Option<u64> {
        process_start_time(pid)
    }

    fn endpoint(&self) -> Result<Endpoint, String> {
        probe_endpoint(&self.paths.endpoint)
    }

    fn read_starting(&self) -> Result<Option<StartingRecord>, String> {
        read_record(&self.paths.starting)
    }

    fn now_unix_ms(&self) -> u64 {
        unix_now_ms()
    }
}

fn frame_or_silence(read: std::io::Result<usize>) -> Endpoint {
    match read {
        Err(error)
            if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
        {
            Endpoint::Silent
        }
        // Data, EOF, or a reset all mean a live process answered the connection.
        _ => Endpoint::Answers,
    }
}

#[cfg(unix)]
fn probe_endpoint(socket: &Path) -> Result<Endpoint, String> {
    use std::io::Read as _;
    use std::os::unix::net::UnixStream;

    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error)
            if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused) =>
        {
            return Ok(Endpoint::Missing)
        }
        Err(error) => return Err(format!("Failed to probe {}: {error}", socket.display())),
    };
    stream
        .set_read_timeout(Some(ENDPOINT_FRAME_WAIT))
        .map_err(|error| format!("Failed to arm endpoint probe: {error}"))?;
    let mut byte = [0u8; 1];
    Ok(frame_or_silence(stream.read(&mut byte)))
}

#[cfg(not(unix))]
fn probe_endpoint(port_file: &Path) -> Result<Endpoint, String> {
    use std::io::Read as _;
    use std::net::{SocketAddr, TcpStream};

    let port = match fs::read_to_string(port_file) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Endpoint::Missing),
        Err(error) => return Err(format!("Failed to read {}: {error}", port_file.display())),
    };
    let Ok(port) = port.trim().parse::<u16>() else {
        return Ok(Endpoint::Missing);
    };
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = match TcpStream::connect_timeout(&address, ENDPOINT_FRAME_WAIT) {
        Ok(stream) => stream,
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
            return Ok(Endpoint::Missing)
        }
        Err(error) => return Err(format!("Failed to probe daemon port {port}: {error}")),
    };
    stream
        .set_read_timeout(Some(ENDPOINT_FRAME_WAIT))
        .map_err(|error| format!("Failed to arm endpoint probe: {error}"))?;
    let mut byte = [0u8; 1];
    Ok(frame_or_silence(stream.read(&mut byte)))
}

/// A held admission.lock. Released on drop.
pub(crate) struct AdmissionGuard {
    _lock: crate::daemon::server::DaemonLockFile,
}

/// Takes admission.lock, retrying until `deadline` passes. Every call opens its own handle, so
/// threads of one process contend exactly like separate processes.
pub(crate) fn acquire_admission_lock(path: &Path, deadline: Duration) -> Result<AdmissionGuard, AdmissionError> {
    let until = Instant::now() + deadline;
    loop {
        let file = crate::daemon::server::open_secure_lock_file(path).map_err(AdmissionError::Io)?;
        if let Ok(lock) = crate::daemon::server::DaemonLockFile::try_lock(file) {
            return Ok(AdmissionGuard { _lock: lock });
        }
        if Instant::now() >= until {
            return Err(AdmissionError::Busy(format!(
                "Another launcher held {} for {deadline:?}",
                path.display()
            )));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    /// admission.lock stayed held past its deadline; nothing was started.
    Busy(String),
    Io(String),
}

impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(message) | Self::Io(message) => f.write_str(message),
        }
    }
}

/// The one start operation admission may issue (job ensure/start/run or a detached spawn).
pub(crate) trait StartRunner {
    fn start(&self) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Admission {
    /// The owner was Absent and exactly one start was issued. `ready` is true when the new
    /// daemon became Alive before the ready timeout.
    Started { ready: bool },
    /// The owner was Alive or Unknown; nothing was started.
    NotStarted(OwnerState),
}

/// Classifies the owner under admission.lock and issues at most one start, only for Absent.
/// The lock stays held after the start until the daemon is Alive or `ready_timeout` passes.
pub(crate) fn admit_and_start(
    paths: &AdmissionPaths,
    probe: &dyn OwnerProbe,
    runner: &dyn StartRunner,
    generation_key: &str,
    lock_deadline: Duration,
    ready_timeout: Duration,
) -> Result<Admission, AdmissionError> {
    let _guard = acquire_admission_lock(&paths.admission_lock, lock_deadline)?;
    let state = classify_owner(probe);
    if state != OwnerState::Absent {
        return Ok(Admission::NotStarted(state));
    }

    let launcher_pid = std::process::id();
    write_record_atomic(
        &paths.starting,
        &StartingRecord {
            launcher_pid,
            launcher_start_time: process_start_time(launcher_pid).unwrap_or(0),
            generation_key: generation_key.to_string(),
            started_at_unix_ms: probe.now_unix_ms(),
        },
    )
    .map_err(AdmissionError::Io)?;
    runner.start().map_err(AdmissionError::Io)?;

    let until = Instant::now() + ready_timeout;
    loop {
        // Probing a free daemon lock briefly takes it, which could make the starting daemon's
        // fail-fast lock attempt lose. The daemon publishes owner.json only after it holds its
        // locks, so the full classification runs only once that record names a live process.
        if owner_record_is_live(probe) && matches!(classify_owner(probe), OwnerState::Alive { .. })
        {
            return Ok(Admission::Started { ready: true });
        }
        if Instant::now() >= until {
            // starting.json stays in place so later callers see a pending start (row 5b).
            return Ok(Admission::Started { ready: false });
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn owner_record_is_live(probe: &dyn OwnerProbe) -> bool {
    matches!(probe.read_record(), Ok(Some(record)) if record_owner_alive(probe, &record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const NOW_MS: u64 = 1_000_000_000;

    struct FakeProbe {
        lock: LockState,
        record: Result<Option<OwnerRecord>, String>,
        processes: HashMap<u32, u64>,
        endpoint: Result<Endpoint, String>,
        starting: Result<Option<StartingRecord>, String>,
        now_ms: u64,
    }

    impl FakeProbe {
        fn new(lock: LockState, endpoint: Endpoint) -> Self {
            Self {
                lock,
                record: Ok(None),
                processes: HashMap::new(),
                endpoint: Ok(endpoint),
                starting: Ok(None),
                now_ms: NOW_MS,
            }
        }

        fn with_record(mut self, pid: u32, start_time: u64) -> Self {
            self.record = Ok(Some(OwnerRecord {
                pid,
                start_time,
                generation_key: "gen-a".to_string(),
                epoch: 1,
            }));
            self
        }

        fn with_process(mut self, pid: u32, start_time: u64) -> Self {
            self.processes.insert(pid, start_time);
            self
        }

        fn with_starting_age_ms(mut self, age_ms: u64) -> Self {
            self.starting = Ok(Some(StartingRecord {
                launcher_pid: 4242,
                launcher_start_time: 7,
                generation_key: "gen-b".to_string(),
                started_at_unix_ms: self.now_ms - age_ms,
            }));
            self
        }
    }

    impl OwnerProbe for FakeProbe {
        fn lock_state(&self) -> LockState {
            self.lock
        }
        fn read_record(&self) -> Result<Option<OwnerRecord>, String> {
            self.record.clone()
        }
        fn process_start_time(&self, pid: u32) -> Option<u64> {
            self.processes.get(&pid).copied()
        }
        fn endpoint(&self) -> Result<Endpoint, String> {
            self.endpoint.clone()
        }
        fn read_starting(&self) -> Result<Option<StartingRecord>, String> {
            self.starting.clone()
        }
        fn now_unix_ms(&self) -> u64 {
            self.now_ms
        }
    }

    fn is_unknown(state: &OwnerState) -> bool {
        matches!(state, OwnerState::Unknown { .. })
    }

    #[test]
    fn row1_lock_error_is_unknown_even_with_alive_record() {
        let probe = FakeProbe::new(LockState::Error, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row1_probe_error_is_unknown() {
        let mut probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        probe.record = Err("corrupt owner.json".to_string());
        assert!(is_unknown(&classify_owner(&probe)));

        let mut probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        probe.endpoint = Err("permission denied".to_string());
        assert!(is_unknown(&classify_owner(&probe)));

        let mut probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        probe.starting = Err("corrupt starting.json".to_string());
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row2_held_with_matching_alive_record_is_alive() {
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100);
        assert_eq!(
            classify_owner(&probe),
            OwnerState::Alive {
                pid: 10,
                start_time: 100
            }
        );
    }

    #[test]
    fn row3_held_without_record_is_unknown() {
        let probe = FakeProbe::new(LockState::Held, Endpoint::Silent);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row3_held_with_reused_pid_is_unknown() {
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 999);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row4_free_lock_with_answering_endpoint_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Answers);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row4_free_lock_with_silent_endpoint_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Silent);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row5_free_missing_with_alive_recorded_owner_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_process(10, 100);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row5b_recent_starting_record_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_starting_age_ms(starting_window_ms() - 1);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row5b_is_evaluated_before_row6_even_with_dead_record() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_starting_age_ms(0);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row6_free_missing_without_record_is_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }

    #[test]
    fn row6_free_missing_with_dead_recorded_pid_is_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing).with_record(10, 100);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }

    #[test]
    fn d3_stale_starting_record_is_ignored_and_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_starting_age_ms(starting_window_ms());
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }

    #[test]
    fn b_lock_held_with_unreadable_holder_pid_is_unknown() {
        // The record names pid 10 but its start time cannot be read.
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers).with_record(10, 100);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn c_alive_pid_with_different_start_time_and_free_lock_is_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_process(10, 555);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }
    // ---- runtime helpers, admission lock, admit_and_start ----

    use std::cell::Cell;
    use std::io::Write as _;

    fn paths_in(dir: &Path) -> AdmissionPaths {
        AdmissionPaths {
            persistent_lock: None,
            legacy_lock: dir.join("daemon.lock"),
            endpoint: dir.join(if cfg!(unix) { "daemon.sock" } else { "daemon.port" }),
            owner: dir.join(OWNER_FILE),
            starting: dir.join(STARTING_FILE),
            admission_lock: dir.join(ADMISSION_LOCK_FILE),
        }
    }

    struct CountingRunner {
        starts: Cell<u32>,
    }

    impl StartRunner for CountingRunner {
        fn start(&self) -> Result<(), String> {
            self.starts.set(self.starts.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn atomic_record_round_trips_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(OWNER_FILE);
        let record = OwnerRecord {
            pid: 1,
            start_time: 2,
            generation_key: "gen".to_string(),
            epoch: 3,
        };
        write_record_atomic(&path, &record).unwrap();
        assert_eq!(read_record::<OwnerRecord>(&path).unwrap(), Some(record));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file must be renamed away");
    }

    #[test]
    fn malformed_record_is_an_error_and_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(OWNER_FILE);
        assert_eq!(read_record::<OwnerRecord>(&path).unwrap(), None);
        fs::write(&path, b"{not json").unwrap();
        assert!(read_record::<OwnerRecord>(&path).is_err());
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn own_process_start_time_is_readable_and_stable() {
        let pid = std::process::id();
        let first = process_start_time(pid).expect("own start time");
        assert_eq!(process_start_time(pid), Some(first));
    }

    #[test]
    fn runtime_probe_on_empty_dir_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        assert_eq!(classify_owner(&RuntimeProbe { paths: &paths }), OwnerState::Absent);
    }

    #[test]
    fn runtime_probe_sees_held_lock_and_live_owner_as_alive() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let file = crate::daemon::server::open_secure_lock_file(&paths.legacy_lock).unwrap();
        let _held = crate::daemon::server::DaemonLockFile::try_lock(file).unwrap();
        publish_owner(&paths, "gen", 9).unwrap();
        assert!(matches!(
            classify_owner(&RuntimeProbe { paths: &paths }),
            OwnerState::Alive { pid, .. } if pid == std::process::id()
        ));
    }

    #[test]
    fn publish_owner_clears_starting_record() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        write_record_atomic(
            &paths.starting,
            &StartingRecord {
                launcher_pid: 1,
                launcher_start_time: 1,
                generation_key: "gen".to_string(),
                started_at_unix_ms: unix_now_ms(),
            },
        )
        .unwrap();
        publish_owner(&paths, "gen", 1).unwrap();
        assert!(!paths.starting.exists());
        assert!(paths.owner.exists());
    }

    #[test]
    fn admission_lock_times_out_as_busy_without_starting() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let _held = acquire_admission_lock(&paths.admission_lock, Duration::ZERO).unwrap();
        let runner = CountingRunner { starts: Cell::new(0) };
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        let result = admit_and_start(
            &paths,
            &probe,
            &runner,
            "gen",
            Duration::from_millis(100),
            Duration::ZERO,
        );
        assert!(matches!(result, Err(AdmissionError::Busy(_))));
        assert_eq!(runner.starts.get(), 0);
    }

    #[test]
    fn c_admit_starts_exactly_once_for_reused_pid() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let runner = CountingRunner { starts: Cell::new(0) };
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_process(10, 555);
        let result = admit_and_start(&paths, &probe, &runner, "gen", Duration::ZERO, Duration::ZERO)
            .unwrap();
        assert_eq!(result, Admission::Started { ready: false });
        assert_eq!(runner.starts.get(), 1);
        assert!(paths.starting.exists(), "a timed-out start leaves starting.json");
    }

    #[test]
    fn b_admit_never_starts_for_unknown_or_alive_owner() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let runner = CountingRunner { starts: Cell::new(0) };
        let unknown = FakeProbe::new(LockState::Held, Endpoint::Answers).with_record(10, 100);
        let alive = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100);
        for probe in [&unknown, &alive] {
            let result =
                admit_and_start(&paths, probe, &runner, "gen", Duration::ZERO, Duration::ZERO)
                    .unwrap();
            assert!(matches!(result, Admission::NotStarted(_)));
        }
        assert_eq!(runner.starts.get(), 0);
        assert!(!paths.starting.exists());
    }

    // ---- (d) two real processes race for admission ----

    const RACE_DIR_ENV: &str = "FERRYX_TEST_ADMISSION_RACE_DIR";
    const RACE_CALLER_ENV: &str = "FERRYX_TEST_ADMISSION_RACE_CALLER";
    const FAKE_DAEMON_ENV: &str = "FERRYX_TEST_ADMISSION_FAKE_DAEMON";
    const RACE_TEST: &str = "daemon::owner_admission::tests::d_two_processes_race_and_start_once";
    const FAKE_DAEMON_TEST: &str = "daemon::owner_admission::tests::d_fake_daemon_entry";
    const BARRIER_WAIT: Duration = Duration::from_secs(60);

    fn wait_for_file(path: &Path) {
        let until = Instant::now() + BARRIER_WAIT;
        while !path.exists() {
            assert!(Instant::now() < until, "timed out waiting for {}", path.display());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn test_binary_command(test_name: &str) -> std::process::Command {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", test_name, "--nocapture", "--test-threads=1"]);
        command
    }

    /// Appends one line per start and launches a fake daemon in its own process.
    struct FakeDaemonRunner {
        dir: PathBuf,
    }

    impl StartRunner for FakeDaemonRunner {
        fn start(&self) -> Result<(), String> {
            let mut log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("starts.log"))
                .map_err(|error| error.to_string())?;
            writeln!(log, "start by {}", std::process::id()).map_err(|error| error.to_string())?;
            test_binary_command(FAKE_DAEMON_TEST)
                .env(FAKE_DAEMON_ENV, &self.dir)
                .env_remove(RACE_DIR_ENV)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }

    /// Fake daemon: takes the daemon lock and publishes owner.json only after release appears,
    /// then stays alive until stop appears. A no-op when run as an ordinary test.
    #[test]
    fn d_fake_daemon_entry() {
        let Some(dir) = std::env::var_os(FAKE_DAEMON_ENV).map(PathBuf::from) else {
            return;
        };
        let paths = paths_in(&dir);
        wait_for_file(&dir.join("release"));
        let file = crate::daemon::server::open_secure_lock_file(&paths.legacy_lock).unwrap();
        let _held = crate::daemon::server::DaemonLockFile::try_lock(file).unwrap();
        publish_owner(&paths, "gen", 1).unwrap();
        wait_for_file(&dir.join("stop"));
    }

    fn run_race_caller(dir: &Path, caller: &str) {
        let paths = paths_in(dir);
        fs::write(dir.join(format!("ready-{caller}")), b"").unwrap();
        // Barrier immediately before admission-lock acquisition.
        wait_for_file(&dir.join("go"));
        let runner = FakeDaemonRunner { dir: dir.to_path_buf() };
        let outcome = admit_and_start(
            &paths,
            &RuntimeProbe { paths: &paths },
            &runner,
            "gen",
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let text = match outcome {
            Ok(Admission::Started { ready }) => format!("started ready={ready}"),
            Ok(Admission::NotStarted(OwnerState::Alive { .. })) => "not-started alive".to_string(),
            Ok(Admission::NotStarted(OwnerState::Unknown { reason })) => {
                format!("not-started unknown {reason}")
            }
            Ok(Admission::NotStarted(OwnerState::Absent)) => "not-started absent".to_string(),
            Err(error) => format!("error {error}"),
        };
        write_record_atomic(&dir.join(format!("outcome-{caller}.json")), &text).unwrap();
    }

    #[test]
    fn d_two_processes_race_and_start_once() {
        if let Some(dir) = std::env::var_os(RACE_DIR_ENV).map(PathBuf::from) {
            let caller = std::env::var(RACE_CALLER_ENV).unwrap();
            run_race_caller(&dir, &caller);
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let mut children: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|caller| {
                test_binary_command(RACE_TEST)
                    .env(RACE_DIR_ENV, dir.path())
                    .env(RACE_CALLER_ENV, caller)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        wait_for_file(&dir.path().join("ready-one"));
        wait_for_file(&dir.path().join("ready-two"));
        fs::write(dir.path().join("go"), b"").unwrap();

        // Delayed startup: the fake daemon only takes its lock once a start is recorded.
        wait_for_file(&dir.path().join("starts.log"));
        fs::write(dir.path().join("release"), b"").unwrap();

        for child in &mut children {
            assert!(child.wait().unwrap().success(), "race caller failed");
        }
        fs::write(dir.path().join("stop"), b"").unwrap();

        let starts = fs::read_to_string(dir.path().join("starts.log")).unwrap();
        assert_eq!(starts.lines().count(), 1, "exactly one start: {starts}");

        let outcomes: Vec<String> = ["one", "two"]
            .iter()
            .map(|caller| {
                read_record(&dir.path().join(format!("outcome-{caller}.json")))
                    .unwrap()
                    .unwrap()
            })
            .collect();
        let started = outcomes.iter().filter(|text| text.starts_with("started")).count();
        assert_eq!(started, 1, "outcomes: {outcomes:?}");
        let other = outcomes
            .iter()
            .find(|text| !text.starts_with("started"))
            .unwrap();
        assert!(
            other == "not-started alive" || other.contains("is pending"),
            "the second caller must classify via row 2 or row 5b: {other}"
        );
    }
}
