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
    /// Nothing answered within the probe window: the connection succeeded but no frame arrived,
    /// or the connect itself did not complete (a Windows loopback connect to a closed port is
    /// refused only after about 2 s, run-r6d rootcause-a-loopback-refusal.txt, so a 500 ms
    /// connect times out). Never proof of an owner: see row 4.
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
    /// The Windows watchdog that spawned this daemon (FERRYX_SUPERVISOR_PID / _START), if any.
    /// Absent in records written by older daemons and by daemons with no watchdog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervisor_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervisor_start_time: Option<u64>,
}

/// The watchdog passes its own pid to the daemon it spawns through this variable.
pub(crate) const SUPERVISOR_PID_ENV: &str = "FERRYX_SUPERVISOR_PID";
/// The watchdog passes its own process start time to the daemon it spawns through this variable.
pub(crate) const SUPERVISOR_START_ENV: &str = "FERRYX_SUPERVISOR_START";

/// The spawning watchdog's identity from the daemon's environment. Both values must parse;
/// a half-set pair is ignored rather than recorded as a partial identity.
pub(crate) fn supervisor_identity_from(env: &dyn Fn(&str) -> Option<String>) -> Option<(u32, u64)> {
    let pid = env(SUPERVISOR_PID_ENV)?.trim().parse().ok()?;
    let start_time = env(SUPERVISOR_START_ENV)?.trim().parse().ok()?;
    Some((pid, start_time))
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
/// 4. Free + endpoint Answers -> Unknown. Free + Silent continues as Missing: the lock is the
///    primary signal (plan todo 1, "primary = the persistent daemon lock ... and the legacy
///    lock"), a live owner always holds it (rows 2/3, evaluated first), and an endpoint that
///    answers nothing is a stale path a retired or killed owner left behind (run-int1 Q08b,
///    run-r6d Q06). Rows 5-5c still refuse a start while a recorded owner, a pending start or
///    a watchdog is alive.
/// 5. Free + Missing + record pid alive with equal start time -> Unknown
///    5b. Free + Missing + starting.json younger than 2 x DAEMON_READY_TIMEOUT -> Unknown
///    5c. (Windows only) Free + Missing + the record's watchdog alive with equal start time ->
///    Unknown: that watchdog may still respawn its daemon, so starting another would race it.
/// 6. Free + Missing otherwise (no record, dead pid, different start time) -> Absent
pub(crate) fn classify_owner(probe: &dyn OwnerProbe) -> OwnerState {
    let state = classify_owner_with(probe, cfg!(windows));
    #[cfg(feature = "local-split-qa")]
    let state = qa_probe_fault::annotate(state);
    state
}

/// [classify_owner] with row 5c switched explicitly, so every target can test both forms.
pub(crate) fn classify_owner_with(probe: &dyn OwnerProbe, watchdog_row: bool) -> OwnerState {
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
    if endpoint == Endpoint::Answers {
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
    // Row 5c.
    if watchdog_row {
        let watchdog = record.as_ref().and_then(|record| {
            let pid = record.supervisor_pid?;
            let start_time = record.supervisor_start_time?;
            (probe.process_start_time(pid) == Some(start_time)).then_some(pid)
        });
        if let Some(pid) = watchdog {
            return OwnerState::Unknown {
                reason: format!("the daemon's watchdog pid {pid} is alive and may respawn it"),
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

    /// Paths for a client bound to `endpoint`. The canonical endpoint uses the runtime paths;
    /// any other endpoint (a test fixture) keeps every file beside it and has no persistent lock.
    pub(crate) fn for_endpoint(endpoint: &Path) -> Self {
        if endpoint == crate::daemon::server::get_socket_path() {
            return Self::for_runtime();
        }
        let dir = endpoint.parent().unwrap_or_else(|| Path::new("."));
        Self {
            persistent_lock: None,
            legacy_lock: dir.join("daemon.lock"),
            endpoint: endpoint.to_path_buf(),
            owner: dir.join(OWNER_FILE),
            starting: dir.join(STARTING_FILE),
            admission_lock: dir.join(ADMISSION_LOCK_FILE),
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

/// The generation key (todo 3, `<version>-<first 8 hex>`) of this process's build, as owner.json,
/// starting.json and control Hello carry it. Resolved once per process; blocking on first use
/// (current_exe, canonicalize, possibly a hash), so async callers resolve it via run_blocking.
pub(crate) fn current_generation_key() -> String {
    static KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        let Ok(exe) = std::env::current_exe() else {
            return env!("CARGO_PKG_VERSION").to_string();
        };
        #[cfg(unix)]
        let exe = crate::daemon::supervisor_linux::daemon_executable(
            std::env::var_os("APPIMAGE").as_deref(),
            &exe,
        );
        generation_key_for(crate::daemon::server::daemon_runtime_root().as_deref(), &exe)
    })
    .clone()
}

/// The generation key of `exe`: a daemon running a staged copy under `runtime_root` is keyed by
/// its generation dir; an unstaged build (a GUI, the detached-fallback daemon) by the key staging
/// it would produce, so starting.json names the generation it starts. Fallback: the package
/// version, when there is no stage source to hash (a dev build without the macOS helper bundle,
/// or an unreadable executable); such a daemon is never a staged generation.
pub(crate) fn generation_key_for(runtime_root: Option<&Path>, exe: &Path) -> String {
    use crate::daemon::handover_request::generation_key_under;
    let staged = runtime_root.and_then(|root| {
        generation_key_under(root, exe).or_else(|| {
            generation_key_under(&fs::canonicalize(root).ok()?, &fs::canonicalize(exe).ok()?)
        })
    });
    staged
        .or_else(|| crate::daemon::runtime_stage::source_generation_key(exe).ok())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
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
    let supervisor = supervisor_identity_from(&|key| std::env::var(key).ok());
    write_record_atomic(
        &paths.owner,
        &OwnerRecord {
            pid,
            start_time,
            generation_key: generation_key.to_string(),
            epoch,
            supervisor_pid: supervisor.map(|(pid, _)| pid),
            supervisor_start_time: supervisor.map(|(_, start_time)| start_time),
        },
    )?;
    withdraw_pending_start(paths)
}

/// Interval between owner.json publication attempts after a failure (todo 8).
pub(crate) const OWNER_PUBLISH_RETRY: Duration = Duration::from_secs(1);

/// Publishes owner.json, retrying every OWNER_PUBLISH_RETRY until one attempt succeeds. Serving
/// never waits on it: until the record names this daemon, locks held + a record naming a dead
/// predecessor classify as row 3 Unknown, so no start is admitted in the meantime. tick is the
/// wait between attempts (a fixed 1 s in production; a test drives it with an event). Returns
/// the number of attempts made.
pub(crate) async fn publish_owner_until_published<P, T, F>(mut publish: P, mut tick: T) -> u32
where
    P: FnMut() -> F,
    F: std::future::Future<Output = Result<(), String>>,
    T: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
{
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        match publish().await {
            Ok(()) => return attempts,
            Err(error) => {
                tracing::warn!(attempts, %error, "Could not publish the daemon owner record; retrying");
            }
        }
        tick().await;
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

/// Deterministic owner-probe faults for QA (todo 12 Q05, feature local-split-qa only).
/// `FERRYX_QA_PROBE_FAULT=inspect-denied|start-time-mismatch|record-unreadable` is read once per
/// process, applied by [RuntimeProbe] to the next classification only, and then cleared. The
/// classification it produced carries the fault name in its Unknown reason, so the GUI's
/// admission log line names both.
#[cfg(feature = "local-split-qa")]
pub(crate) mod qa_probe_fault {
    use super::OwnerState;
    use std::cell::Cell;
    use std::sync::{Mutex, OnceLock};

    pub(crate) const PROBE_FAULT_ENV: &str = "FERRYX_QA_PROBE_FAULT";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ProbeFault {
        /// The lock state cannot be inspected (row 1).
        InspectDenied,
        /// The recorded owner pid's start time differs from owner.json.
        StartTimeMismatch,
        /// owner.json cannot be read.
        RecordUnreadable,
    }

    impl ProbeFault {
        pub(crate) fn parse(raw: &str) -> Option<Self> {
            match raw.trim() {
                "inspect-denied" => Some(Self::InspectDenied),
                "start-time-mismatch" => Some(Self::StartTimeMismatch),
                "record-unreadable" => Some(Self::RecordUnreadable),
                _ => None,
            }
        }

        pub(crate) fn as_str(self) -> &'static str {
            match self {
                Self::InspectDenied => "inspect-denied",
                Self::StartTimeMismatch => "start-time-mismatch",
                Self::RecordUnreadable => "record-unreadable",
            }
        }
    }

    /// The process's one fault, taken by the first classification that reads it.
    fn pending() -> &'static Mutex<Option<ProbeFault>> {
        static PENDING: OnceLock<Mutex<Option<ProbeFault>>> = OnceLock::new();
        PENDING.get_or_init(|| {
            let raw = std::env::var(PROBE_FAULT_ENV).ok();
            let fault = raw.as_deref().and_then(ProbeFault::parse);
            if let (Some(raw), None) = (raw.as_deref(), fault) {
                tracing::warn!("{PROBE_FAULT_ENV}={raw} is not a known probe fault; ignored");
            }
            Mutex::new(fault)
        })
    }

    thread_local! {
        /// The fault applied to the classification running on this thread.
        static ACTIVE: Cell<Option<ProbeFault>> = const { Cell::new(None) };
    }

    /// Called at the start of a classification (the lock read): takes the pending fault, once.
    pub(crate) fn begin_classification() -> Option<ProbeFault> {
        if let Some(fault) = pending().lock().ok().and_then(|mut pending| pending.take()) {
            ACTIVE.with(|active| active.set(Some(fault)));
        }
        active()
    }

    pub(crate) fn active() -> Option<ProbeFault> {
        ACTIVE.with(Cell::get)
    }

    /// Ends the classification: clears the fault and names it in the Unknown it produced.
    pub(crate) fn annotate(state: OwnerState) -> OwnerState {
        let Some(fault) = ACTIVE.with(|active| active.replace(None)) else {
            return state;
        };
        match state {
            OwnerState::Unknown { reason } => OwnerState::Unknown {
                reason: format!("{reason} ({PROBE_FAULT_ENV}={})", fault.as_str()),
            },
            other => other,
        }
    }

    /// Test seam: applies `fault` to the next classification on this thread without the env.
    #[cfg(test)]
    pub(crate) fn arm_on_this_thread(fault: ProbeFault) {
        ACTIVE.with(|active| active.set(Some(fault)));
    }
    /// QA-only forced fallback (todo 12 Q06). `FERRYX_QA_FORCE_FALLBACK=1` on a QA GUI makes
    /// both of its start paths skip the OS job: GUI setup issues no supervised start, and
    /// `DaemonClient::connect_or_spawn` goes straight to the detached fallback spawn. Admission
    /// is unchanged, so the forced launcher still starts only on Absent, under admission.lock.
    pub(crate) const FORCE_FALLBACK_ENV: &str = "FERRYX_QA_FORCE_FALLBACK";

    pub(crate) fn force_fallback_from(value: Option<&str>) -> bool {
        value.is_some_and(|value| value.trim() == "1")
    }

    pub(crate) fn force_fallback() -> bool {
        force_fallback_from(std::env::var(FORCE_FALLBACK_ENV).ok().as_deref())
    }
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
        // Every classification reads the lock first, so this is where the one-shot QA fault
        // (if any) is taken for the classification that follows.
        #[cfg(feature = "local-split-qa")]
        if qa_probe_fault::begin_classification() == Some(qa_probe_fault::ProbeFault::InspectDenied) {
            return LockState::Error;
        }
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
        #[cfg(feature = "local-split-qa")]
        if qa_probe_fault::active() == Some(qa_probe_fault::ProbeFault::RecordUnreadable) {
            return Err(format!("{}=record-unreadable", qa_probe_fault::PROBE_FAULT_ENV));
        }
        read_record(&self.paths.owner)
    }

    fn process_start_time(&self, pid: u32) -> Option<u64> {
        let start_time = process_start_time(pid);
        #[cfg(feature = "local-split-qa")]
        if qa_probe_fault::active() == Some(qa_probe_fault::ProbeFault::StartTimeMismatch) {
            return start_time.map(|start| start.wrapping_add(1));
        }
        start_time
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

/// Endpoint for a connect that failed: refused or absent is Missing, a connect that did not
/// complete within the probe window is Silent (nothing answered). Anything else is a probe error.
fn unanswered_connect(kind: ErrorKind) -> Option<Endpoint> {
    match kind {
        ErrorKind::NotFound | ErrorKind::ConnectionRefused => Some(Endpoint::Missing),
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Some(Endpoint::Silent),
        _ => None,
    }
}

#[cfg(unix)]
fn probe_endpoint(socket: &Path) -> Result<Endpoint, String> {
    use std::io::Read as _;
    use std::os::unix::net::UnixStream;

    let mut stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(error) => {
            return unanswered_connect(error.kind())
                .ok_or_else(|| format!("Failed to probe {}: {error}", socket.display()))
        }
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
        Err(error) => {
            return unanswered_connect(error.kind())
                .ok_or_else(|| format!("Failed to probe daemon port {port}: {error}"))
        }
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
    if let Some(parent) = path.parent() {
        crate::daemon::server::ensure_runtime_directory(parent).map_err(AdmissionError::Io)?;
    }
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
/// Test seam only: production start sites take [`admit`] and issue their own start.
#[cfg(test)]
pub(crate) trait StartRunner {
    fn start(&self) -> Result<(), String>;
}

/// Result of taking admission for one start.
pub(crate) enum Admit {
    /// The owner was Absent: starting.json is written and the caller may issue exactly one
    /// start while it keeps the guard, until the new daemon is Alive or the ready timeout passes.
    Start(AdmissionGuard),
    /// The owner was Alive or Unknown; the caller must not start, reload, or replace anything.
    Refuse(OwnerState),
}

/// Takes admission.lock and classifies the owner under it. For an Absent owner it also records
/// the pending start in starting.json before returning, so the caller only has to start.
pub(crate) fn admit(
    paths: &AdmissionPaths,
    probe: &dyn OwnerProbe,
    generation_key: &str,
    lock_deadline: Duration,
) -> Result<Admit, AdmissionError> {
    let guard = acquire_admission_lock(&paths.admission_lock, lock_deadline)?;
    let state = classify_owner(probe);
    if state != OwnerState::Absent {
        return Ok(Admit::Refuse(state));
    }
    record_pending_start(paths, generation_key, probe.now_unix_ms()).map_err(AdmissionError::Io)?;
    Ok(Admit::Start(guard))
}

/// Writes starting.json for a start this process is about to issue.
pub(crate) fn record_pending_start(
    paths: &AdmissionPaths,
    generation_key: &str,
    now_unix_ms: u64,
) -> Result<(), String> {
    let launcher_pid = std::process::id();
    write_record_atomic(
        &paths.starting,
        &StartingRecord {
            launcher_pid,
            launcher_start_time: process_start_time(launcher_pid).unwrap_or(0),
            generation_key: generation_key.to_string(),
            started_at_unix_ms: now_unix_ms,
        },
    )
}

#[cfg(test)]
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
#[cfg(test)]
pub(crate) fn admit_and_start(
    paths: &AdmissionPaths,
    probe: &dyn OwnerProbe,
    runner: &dyn StartRunner,
    generation_key: &str,
    lock_deadline: Duration,
    ready_timeout: Duration,
) -> Result<Admission, AdmissionError> {
    let _guard = match admit(paths, probe, generation_key, lock_deadline)? {
        Admit::Start(guard) => guard,
        Admit::Refuse(state) => return Ok(Admission::NotStarted(state)),
    };
    runner.start().map_err(AdmissionError::Io)?;
    Ok(Admission::Started {
        ready: wait_until_alive(probe, ready_timeout),
    })
}

/// Waits for the started daemon to become Alive (truth-table row 2). A timeout leaves
/// starting.json in place so later callers see a pending start (row 5b).
pub(crate) fn wait_until_alive(probe: &dyn OwnerProbe, ready_timeout: Duration) -> bool {
    let until = Instant::now() + ready_timeout;
    loop {
        // Probing a free daemon lock briefly takes it, which could make the starting daemon's
        // fail-fast lock attempt lose. The daemon publishes owner.json only after it holds its
        // locks, so the full classification runs only once that record names a live process.
        if owner_record_is_live(probe) && matches!(classify_owner(probe), OwnerState::Alive { .. })
        {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// The supervisor operations GUI setup may issue, injected so tests need no real
/// launchctl/systemctl/schtasks.
pub(crate) trait SupervisorOps {
    /// Ensures and starts the supervised job. Only called for an Absent owner under admission.
    /// Returns whether a start was actually issued.
    fn start_absent_owner(&self) -> Result<bool, String>;
    /// Migrates the legacy single job (todo 14) for the owner admission just classified, while
    /// admission.lock is held. Alive or Unknown: the legacy job keeps running and only loses its
    /// login autostart. Absent: the legacy job is removed. Never stops a running daemon.
    fn migrate_legacy_job(&self, owner: &OwnerState) -> Result<(), String>;
    /// Registers this build for the next login without touching any running job.
    fn reconcile_login_registration(&self) -> Result<(), String>;
}

/// GUI setup: under admission.lock it classifies the owner once, migrates the legacy single job
/// for that classification, and starts the supervised daemon only when the owner is Absent; then
/// it reconciles login registration. Returns whether a start was issued. An Alive or Unknown
/// owner is never reloaded, restarted, or stopped. A failed legacy migration is logged and never
/// blocks the start.
pub(crate) fn ensure_if_absent_with(
    paths: &AdmissionPaths,
    probe: &dyn OwnerProbe,
    ops: &dyn SupervisorOps,
    lock_deadline: Duration,
    ready_timeout: Duration,
) -> Result<bool, String> {
    let guard =
        acquire_admission_lock(&paths.admission_lock, lock_deadline).map_err(|error| error.to_string())?;
    let state = classify_owner(probe);
    // A legacy daemon shares the runtime dir, locks and endpoint, so this classification is the
    // legacy owner's too; migrating under the same lock keeps it consistent with the start below.
    if let Err(error) = ops.migrate_legacy_job(&state) {
        tracing::warn!(%error, ?state, "Could not migrate the legacy daemon job; nothing was stopped");
    }
    let started = if state == OwnerState::Absent {
        record_pending_start(paths, &current_generation_key(), probe.now_unix_ms())?;
        match ops.start_absent_owner() {
            Ok(true) => {
                if !wait_until_alive(probe, ready_timeout) {
                    tracing::warn!("The supervised daemon did not become ready in time");
                }
                true
            }
            Ok(false) => {
                withdraw_pending_start(paths)?;
                false
            }
            Err(error) => {
                withdraw_pending_start(paths)?;
                return Err(error);
            }
        }
    } else {
        tracing::info!(?state, "A daemon owner exists; GUI setup starts nothing");
        false
    };
    drop(guard);
    // Registration runs after any start so it cannot make the start look like a no-op change.
    ops.reconcile_login_registration()?;
    Ok(started)
}

/// Removes this caller's starting.json after it issued no start, so its own marker never makes
/// the next classification report a pending start (row 5b).
pub(crate) fn withdraw_pending_start(paths: &AdmissionPaths) -> Result<(), String> {
    match fs::remove_file(&paths.starting) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove {}: {error}", paths.starting.display())),
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
                supervisor_pid: None,
                supervisor_start_time: None,
            }));
            self
        }

        fn with_watchdog(mut self, pid: u32, start_time: u64) -> Self {
            if let Ok(Some(record)) = &mut self.record {
                record.supervisor_pid = Some(pid);
                record.supervisor_start_time = Some(start_time);
            }
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
    fn row4_free_lock_with_silent_endpoint_is_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Silent);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }

    /// A silent endpoint never outranks the lock: held stays Alive or Unknown, and a free lock
    /// still honours rows 5 and 5b.
    #[test]
    fn silent_endpoint_is_missing_only_behind_a_free_lock() {
        let held_alive = FakeProbe::new(LockState::Held, Endpoint::Silent)
            .with_record(10, 100)
            .with_process(10, 100);
        assert_eq!(classify_owner(&held_alive), OwnerState::Alive { pid: 10, start_time: 100 });
        assert!(is_unknown(&classify_owner(&FakeProbe::new(LockState::Held, Endpoint::Silent))));
        let free_alive_record = FakeProbe::new(LockState::Free, Endpoint::Silent)
            .with_record(10, 100)
            .with_process(10, 100);
        assert!(is_unknown(&classify_owner(&free_alive_record)));
        let pending = FakeProbe::new(LockState::Free, Endpoint::Silent).with_starting_age_ms(0);
        assert!(is_unknown(&classify_owner(&pending)));
    }

    /// A connect that does not complete in the window (the Windows loopback case) is Silent, not
    /// a probe error: a probe error is row 1 Unknown and blocks every start for good.
    #[test]
    fn timed_out_connect_is_silent_and_refused_is_missing() {
        assert_eq!(unanswered_connect(ErrorKind::TimedOut), Some(Endpoint::Silent));
        assert_eq!(unanswered_connect(ErrorKind::WouldBlock), Some(Endpoint::Silent));
        assert_eq!(unanswered_connect(ErrorKind::ConnectionRefused), Some(Endpoint::Missing));
        assert_eq!(unanswered_connect(ErrorKind::NotFound), Some(Endpoint::Missing));
        assert_eq!(unanswered_connect(ErrorKind::PermissionDenied), None);
    }

    /// The run-int1 Q08b shape on a real socket: a bound listener that nobody accepts on. Behind
    /// a free lock it is Absent (a start is allowed); behind a held lock it is never Absent.
    #[cfg(unix)]
    #[test]
    fn stale_listener_that_never_answers_admits_only_while_the_lock_is_free() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let _listener = std::os::unix::net::UnixListener::bind(&paths.endpoint).unwrap();
        let probe = RuntimeProbe { paths: &paths };
        assert_eq!(probe.endpoint().unwrap(), Endpoint::Silent);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
        let file = crate::daemon::server::open_secure_lock_file(&paths.legacy_lock).unwrap();
        let _held = crate::daemon::server::DaemonLockFile::try_lock(file).unwrap();
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

    #[test]
    fn row5c_alive_watchdog_of_a_dead_daemon_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_watchdog(20, 200)
            .with_process(20, 200);
        assert!(is_unknown(&classify_owner_with(&probe, true)));
    }

    #[test]
    fn row5c_applies_only_where_a_watchdog_exists() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_watchdog(20, 200)
            .with_process(20, 200);
        assert_eq!(classify_owner_with(&probe, false), OwnerState::Absent);
    }

    #[test]
    fn row5c_dead_or_reused_watchdog_pid_is_absent() {
        let dead = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_watchdog(20, 200);
        assert_eq!(classify_owner_with(&dead, true), OwnerState::Absent);
        let reused = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_watchdog(20, 200)
            .with_process(20, 999);
        assert_eq!(classify_owner_with(&reused, true), OwnerState::Absent);
    }

    #[test]
    fn row5c_never_overrides_an_alive_owner() {
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100)
            .with_watchdog(20, 200)
            .with_process(20, 200);
        assert_eq!(
            classify_owner_with(&probe, true),
            OwnerState::Alive { pid: 10, start_time: 100 }
        );
    }

    #[test]
    fn supervisor_identity_needs_both_values() {
        let both = |key: &str| match key {
            SUPERVISOR_PID_ENV => Some("42".to_string()),
            SUPERVISOR_START_ENV => Some("133700".to_string()),
            _ => None,
        };
        assert_eq!(supervisor_identity_from(&both), Some((42, 133700)));
        let pid_only = |key: &str| (key == SUPERVISOR_PID_ENV).then(|| "42".to_string());
        assert_eq!(supervisor_identity_from(&pid_only), None);
        let garbage = |_: &str| Some("x".to_string());
        assert_eq!(supervisor_identity_from(&garbage), None);
    }

    #[test]
    fn owner_record_without_watchdog_fields_still_parses() {
        let record: OwnerRecord =
            serde_json::from_str(r#"{"pid":1,"startTime":2,"generationKey":"g","epoch":3}"#).unwrap();
        assert_eq!(record.supervisor_pid, None);
        assert_eq!(record.supervisor_start_time, None);
        let written = serde_json::to_string(&record).unwrap();
        assert!(!written.contains("supervisor"), "{written}");
    }
    // ---- runtime helpers, admission lock, admit_and_start ----

    use std::cell::Cell;

    #[test]
    fn generation_key_of_a_staged_executable_is_its_generation_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("daemon-runtime");
        let exe = root.join("2026.1013.2-0a1b2c3d").join("ferryx");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"staged").unwrap();
        assert_eq!(generation_key_for(Some(&root), &exe), "2026.1013.2-0a1b2c3d");
    }

    /// An unstaged single-file build (Linux, Windows, an AppImage) is keyed like the generation it
    /// stages: <version>-<first 8 hex of its sha256>, so starting.json names the job it starts.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn generation_key_of_an_unstaged_executable_is_the_key_it_stages() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app").join("ferryx");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"unstaged build").unwrap();
        let hash: [u8; 32] = Sha256::digest(b"unstaged build").into();
        let expected = crate::daemon::runtime_stage::generation_key(env!("CARGO_PKG_VERSION"), &hash);
        assert_eq!(generation_key_for(Some(&dir.path().join("daemon-runtime")), &exe), expected);
    }

    /// With no stage source to hash (a dev build without the macOS helper bundle, an unreadable
    /// executable) the key falls back to the package version.
    #[test]
    fn generation_key_without_a_stage_source_falls_back_to_the_version() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone").join("ferryx");
        assert_eq!(generation_key_for(None, &missing), env!("CARGO_PKG_VERSION"));
    }

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
            supervisor_pid: Some(4),
            supervisor_start_time: Some(5),
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

    #[cfg(feature = "local-split-qa")]
    mod qa_probe_fault_tests {
        use super::super::qa_probe_fault::{arm_on_this_thread, ProbeFault, PROBE_FAULT_ENV};
        use super::*;

        fn held_alive_owner(dir: &Path) -> (AdmissionPaths, crate::daemon::server::DaemonLockFile) {
            let paths = paths_in(dir);
            let file = crate::daemon::server::open_secure_lock_file(&paths.legacy_lock).unwrap();
            let held = crate::daemon::server::DaemonLockFile::try_lock(file).unwrap();
            publish_owner(&paths, "gen", 9).unwrap();
            (paths, held)
        }

        #[test]
        fn force_fallback_needs_exactly_one() {
            use super::super::qa_probe_fault::force_fallback_from;
            assert!(force_fallback_from(Some("1")));
            assert!(force_fallback_from(Some(" 1 ")));
            assert!(!force_fallback_from(None));
            assert!(!force_fallback_from(Some("0")));
            assert!(!force_fallback_from(Some("true")));
        }

        #[test]
        fn probe_fault_names_parse_and_round_trip() {
            for name in ["inspect-denied", "start-time-mismatch", "record-unreadable"] {
                assert_eq!(ProbeFault::parse(name).unwrap().as_str(), name);
            }
            assert_eq!(ProbeFault::parse("bogus"), None);
        }

        /// Each fault turns an Alive owner into a typed Unknown naming the fault, and only for
        /// the one classification it was applied to.
        #[test]
        fn each_fault_yields_unknown_once_then_clears() {
            for fault in [ProbeFault::InspectDenied, ProbeFault::StartTimeMismatch, ProbeFault::RecordUnreadable] {
                let dir = tempfile::tempdir().unwrap();
                let (paths, _held) = held_alive_owner(dir.path());
                let probe = RuntimeProbe { paths: &paths };
                arm_on_this_thread(fault);
                match classify_owner(&probe) {
                    OwnerState::Unknown { reason } => {
                        let tag = format!("{PROBE_FAULT_ENV}={}", fault.as_str());
                        assert!(reason.contains(&tag), "{reason}");
                        assert!(format!("{:?}", OwnerState::Unknown { reason: reason.clone() }).contains(fault.as_str()));
                    }
                    other => panic!("{fault:?} must classify Unknown, got {other:?}"),
                }
                assert!(
                    matches!(classify_owner(&probe), OwnerState::Alive { pid, .. } if pid == std::process::id()),
                    "{fault:?} must apply to one classification only"
                );
            }
        }

        #[test]
        fn no_fault_leaves_classification_unchanged() {
            let dir = tempfile::tempdir().unwrap();
            let (paths, _held) = held_alive_owner(dir.path());
            assert!(matches!(classify_owner(&RuntimeProbe { paths: &paths }), OwnerState::Alive { .. }));
        }
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

    #[test]
    fn f_fallback_spawn_is_refused_for_alive_owner() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100);
        let admitted = admit(&paths, &probe, "gen", Duration::ZERO).unwrap();
        assert!(matches!(admitted, Admit::Refuse(OwnerState::Alive { pid: 10, .. })));
        assert!(!paths.starting.exists(), "a refused admission records no pending start");
    }

    #[test]
    fn admit_records_pending_start_only_for_absent_owner() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        let admitted = admit(&paths, &probe, "gen", Duration::ZERO).unwrap();
        assert!(matches!(admitted, Admit::Start(_)));
        let starting: StartingRecord = read_record(&paths.starting).unwrap().unwrap();
        assert_eq!(starting.launcher_pid, std::process::id());
        assert_eq!(starting.started_at_unix_ms, NOW_MS);
    }

    #[derive(Default)]
    struct RecordingOps {
        starts: Cell<u32>,
        registrations: Cell<u32>,
        migrations: std::cell::RefCell<Vec<OwnerState>>,
    }

    /// todo 8: a committed successor whose owner.json publication fails for the first 3
    /// attempts keeps serving while classify_owner reports Unknown (locks held + a record
    /// naming a dead predecessor, row 3) and admission starts nothing; the 4th attempt
    /// publishes and the probe reports Alive with this pid. Each retry tick is an event the
    /// test releases, never a sleep.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn owner_publication_retries_until_published_and_admits_nothing_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        // Inherited locks: held by this process. The record names a dead predecessor.
        let _locks = crate::daemon::server::acquire_daemon_locks(None, &paths.legacy_lock).unwrap();
        write_record_atomic(
            &paths.owner,
            &OwnerRecord {
                pid: u32::MAX - 7,
                start_time: 1,
                generation_key: "gen-predecessor".into(),
                epoch: 1,
                supervisor_pid: None,
                supervisor_start_time: None,
            },
        )
        .unwrap();

        let (tick_tx, tick_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let tick_rx = std::sync::Arc::new(tokio::sync::Mutex::new(tick_rx));
        let (attempt_tx, mut attempts) = tokio::sync::mpsc::unbounded_channel::<u32>();
        let failures = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(3));
        let publisher = {
            let paths = paths.clone();
            let failures = failures.clone();
            let tick_rx = tick_rx.clone();
            tokio::spawn(async move {
                let mut attempt = 0u32;
                publish_owner_until_published(
                    || {
                        attempt += 1;
                        let _ = attempt_tx.send(attempt);
                        let mut left = failures.load(std::sync::atomic::Ordering::SeqCst);
                        let failing = loop {
                            let Some(next) = left.checked_sub(1) else { break false };
                            match failures.compare_exchange_weak(
                                left,
                                next,
                                std::sync::atomic::Ordering::SeqCst,
                                std::sync::atomic::Ordering::SeqCst,
                            ) {
                                Ok(_) => break true,
                                Err(current) => left = current,
                            }
                        };
                        let paths = paths.clone();
                        async move {
                            if failing {
                                Err("injected owner.json failure".to_string())
                            } else {
                                publish_owner(&paths, "gen-successor", 2)
                            }
                        }
                    },
                    || -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
                        let tick_rx = tick_rx.clone();
                        Box::pin(async move {
                            let _ = tick_rx.lock().await.recv().await;
                        })
                    },
                )
                .await
            })
        };

        for failed in 1..=3u32 {
            assert_eq!(attempts.recv().await, Some(failed));
            let probe = RuntimeProbe { paths: &paths };
            assert!(
                matches!(classify_owner(&probe), OwnerState::Unknown { .. }),
                "attempt {failed}: an unpublished successor must classify Unknown"
            );
            let ops = RecordingOps::default();
            ensure_if_absent_with(&paths, &probe, &ops, Duration::ZERO, Duration::ZERO).unwrap();
            assert_eq!(ops.starts.get(), 0, "attempt {failed}: a start was admitted");
            tick_tx.send(()).unwrap();
        }
        assert_eq!(attempts.recv().await, Some(4));
        assert_eq!(publisher.await.unwrap(), 4);
        let probe = RuntimeProbe { paths: &paths };
        let pid = std::process::id();
        assert!(
            matches!(classify_owner(&probe), OwnerState::Alive { pid: alive, .. } if alive == pid),
            "the 4th attempt must publish this daemon as the Alive owner"
        );
    }

    impl SupervisorOps for RecordingOps {
        fn start_absent_owner(&self) -> Result<bool, String> {
            self.starts.set(self.starts.get() + 1);
            Ok(true)
        }
        fn migrate_legacy_job(&self, owner: &OwnerState) -> Result<(), String> {
            self.migrations.borrow_mut().push(owner.clone());
            Ok(())
        }
        fn reconcile_login_registration(&self) -> Result<(), String> {
            self.registrations.set(self.registrations.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn e_gui_setup_with_alive_owner_only_reconciles_registration() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let ops = RecordingOps::default();
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100);
        let started =
            ensure_if_absent_with(&paths, &probe, &ops, Duration::ZERO, Duration::ZERO).unwrap();
        assert!(!started);
        assert_eq!(ops.starts.get(), 0, "an alive owner's job is never started or reloaded");
        assert_eq!(ops.registrations.get(), 1);
        assert_eq!(
            *ops.migrations.borrow(),
            [OwnerState::Alive { pid: 10, start_time: 100 }],
            "the legacy job is migrated once, for the alive owner"
        );
        assert!(!paths.starting.exists());
    }

    #[test]
    fn e_gui_setup_with_unknown_owner_starts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let ops = RecordingOps::default();
        let probe = FakeProbe::new(LockState::Free, Endpoint::Answers);
        assert!(!ensure_if_absent_with(&paths, &probe, &ops, Duration::ZERO, Duration::ZERO).unwrap());
        assert_eq!(ops.starts.get(), 0);
        assert_eq!(ops.registrations.get(), 1);
    }

    #[test]
    fn gui_setup_with_absent_owner_starts_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let ops = RecordingOps::default();
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        assert!(ensure_if_absent_with(&paths, &probe, &ops, Duration::ZERO, Duration::ZERO).unwrap());
        assert_eq!(ops.starts.get(), 1);
        assert_eq!(ops.registrations.get(), 1);
    }

    #[test]
    fn a_silent_endpoint_with_held_lock_issues_no_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let runner = CountingRunner { starts: Cell::new(0) };
        let probe = FakeProbe::new(LockState::Held, Endpoint::Silent);
        let result =
            admit_and_start(&paths, &probe, &runner, "gen", Duration::ZERO, Duration::ZERO).unwrap();
        assert!(matches!(result, Admission::NotStarted(OwnerState::Unknown { .. })));
        assert_eq!(runner.starts.get(), 0);
    }

    /// Records the order of supervisor operations and whether admission.lock was held during the
    /// legacy migration (another handle must fail to take it).
    struct OrderingOps {
        admission_lock: PathBuf,
        events: std::cell::RefCell<Vec<String>>,
        fail_migration: bool,
    }

    impl SupervisorOps for OrderingOps {
        fn start_absent_owner(&self) -> Result<bool, String> {
            self.events.borrow_mut().push("start".to_string());
            Ok(true)
        }
        fn migrate_legacy_job(&self, owner: &OwnerState) -> Result<(), String> {
            let held = matches!(
                acquire_admission_lock(&self.admission_lock, Duration::ZERO),
                Err(AdmissionError::Busy(_))
            );
            let kind = match owner {
                OwnerState::Alive { .. } => "alive",
                OwnerState::Unknown { .. } => "unknown",
                OwnerState::Absent => "absent",
            };
            self.events
                .borrow_mut()
                .push(format!("migrate:{kind}:locked={held}"));
            if self.fail_migration {
                return Err("legacy migration failed".to_string());
            }
            Ok(())
        }
        fn reconcile_login_registration(&self) -> Result<(), String> {
            self.events.borrow_mut().push("register".to_string());
            Ok(())
        }
    }

    /// Todo 14: the legacy job is migrated exactly once per GUI setup, under admission.lock, with
    /// the owner that lock's classification produced; it precedes any start, and a failed
    /// migration never blocks the newest generation's start or login registration.
    #[test]
    fn legacy_migration_runs_under_admission_with_the_classified_owner() {
        let cases: Vec<(FakeProbe, Vec<&str>)> = vec![
            (
                FakeProbe::new(LockState::Held, Endpoint::Answers)
                    .with_record(10, 100)
                    .with_process(10, 100),
                vec!["migrate:alive:locked=true", "register"],
            ),
            (
                FakeProbe::new(LockState::Free, Endpoint::Answers),
                vec!["migrate:unknown:locked=true", "register"],
            ),
            (
                FakeProbe::new(LockState::Free, Endpoint::Missing),
                vec!["migrate:absent:locked=true", "start", "register"],
            ),
        ];
        for fail_migration in [false, true] {
            for (probe, expected) in &cases {
                let dir = tempfile::tempdir().unwrap();
                let paths = paths_in(dir.path());
                let ops = OrderingOps {
                    admission_lock: paths.admission_lock.clone(),
                    events: Default::default(),
                    fail_migration,
                };
                let started =
                    ensure_if_absent_with(&paths, probe, &ops, Duration::ZERO, Duration::ZERO)
                        .unwrap();
                assert_eq!(started, expected.contains(&"start"));
                assert_eq!(*ops.events.borrow(), *expected, "fail_migration={fail_migration}");
                assert!(
                    acquire_admission_lock(&paths.admission_lock, Duration::ZERO).is_ok(),
                    "admission.lock is released after GUI setup"
                );
            }
        }
    }
}
