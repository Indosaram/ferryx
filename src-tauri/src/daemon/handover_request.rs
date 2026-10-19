//! The handover request a predecessor writes before it asks its successor generation's OS job to
//! start (todo 8). It is the only authority under which a daemon may start while an owner is
//! Alive: the successor runs as a HandoverCandidate only when the request names its own
//! generation, has not expired and names a predecessor that is still the same live process.
//! Anything else makes the starting daemon delete the request and exit 0 without taking a lock.
#![cfg_attr(windows, allow(dead_code))]

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::daemon::owner_admission::{acquire_admission_lock, read_record, write_record_atomic};

/// File name inside the runtime dir.
pub(crate) const REQUEST_FILE: &str = "handover-request.json";
/// How long a written request authorizes a candidate start.
pub(crate) const REQUEST_TTL_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HandoverRequest {
    pub legacy_socket_path: PathBuf,
    pub predecessor_pid: u32,
    pub predecessor_start_time: u64,
    pub predecessor_epoch: u64,
    pub generation_key: String,
    /// The staged executable the successor generation's job runs (todo 3).
    pub staged_executable: PathBuf,
    /// 32 random bytes, hex.
    pub nonce: String,
    pub expires_at_unix_ms: u64,
}

impl HandoverRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        legacy_socket_path: PathBuf,
        predecessor_pid: u32,
        predecessor_start_time: u64,
        predecessor_epoch: u64,
        generation_key: String,
        staged_executable: PathBuf,
        now_unix_ms: u64,
    ) -> Self {
        let nonce_bytes: [u8; 32] = rand::random();
        Self {
            legacy_socket_path,
            predecessor_pid,
            predecessor_start_time,
            predecessor_epoch,
            generation_key,
            staged_executable,
            nonce: nonce_bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
            expires_at_unix_ms: now_unix_ms.saturating_add(REQUEST_TTL_MS),
        }
    }
}

pub(crate) fn request_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(REQUEST_FILE)
}

/// The generation key of the running executable when it is a staged runtime copy, else None
/// (a dev build or an unstaged install).
pub(crate) fn own_generation_key() -> Option<String> {
    let root = crate::daemon::server::daemon_runtime_root()?;
    let exe = std::env::current_exe().ok()?;
    generation_key_under(&root, &exe).or_else(|| {
        let root = std::fs::canonicalize(&root).ok()?;
        let exe = std::fs::canonicalize(&exe).ok()?;
        generation_key_under(&root, &exe)
    })
}

/// Counts TransferSessions requests this process has served. The predecessor snapshots it when it
/// writes a request; an unchanged count once the request has expired means no candidate ever
/// started a transfer, so the predecessor may abandon the prepared handover on its own.
static TRANSFERS_STARTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn note_transfer_started() {
    TRANSFERS_STARTED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn transfers_started() -> u64 {
    TRANSFERS_STARTED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Writes the request atomically with mode 0600.
pub(crate) fn write_request(runtime_dir: &Path, request: &HandoverRequest) -> Result<(), String> {
    write_record_atomic(&request_path(runtime_dir), request)
}

pub(crate) fn remove_request(runtime_dir: &Path) {
    let path = request_path(runtime_dir);
    if let Err(error) = std::fs::remove_file(&path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(%error, path = %path.display(), "Could not remove the handover request");
        }
    }
}

/// The generation key of an executable staged under `root`: the first path component below the
/// root. None when the executable is not strictly inside a generation dir of `root`.
pub(crate) fn generation_key_under(root: &Path, executable: &Path) -> Option<String> {
    let relative = executable.strip_prefix(root).ok()?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let mut components = relative.components();
    let key = match components.next()? {
        Component::Normal(name) => name.to_str()?.to_string(),
        _ => return None,
    };
    components.next()?;
    Some(key)
}

/// Check (a) of todo 8: the staged executable lies under the daemon runtime root, inside the
/// generation dir named by `generation_key`, and exists.
pub(crate) fn validate_staged_executable(
    root: &Path,
    executable: &Path,
    generation_key: &str,
) -> Result<(), String> {
    match generation_key_under(root, executable) {
        Some(key) if key == generation_key => {}
        Some(key) => {
            return Err(format!(
                "staged executable belongs to generation {key}, not {generation_key}"
            ))
        }
        None => {
            return Err(format!(
                "{} is not inside the daemon runtime root {}",
                executable.display(),
                root.display()
            ))
        }
    }
    if !executable.is_file() {
        return Err(format!("staged executable {} does not exist", executable.display()));
    }
    Ok(())
}

/// What a daemon started with plain `--daemon` does, decided from the request on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupRole {
    /// No request: an ordinary start.
    Ordinary,
    /// Run the existing `--handover-from` path against the predecessor's legacy socket.
    /// The request's expiry also bounds the transfer: a candidate that has not sent
    /// CommitHandover by then aborts and exits 0 (todo 8).
    Candidate { legacy_socket_path: PathBuf, expires_at_unix_ms: u64 },
    /// Invalid, expired or foreign request: exit 0 without taking any lock.
    Exit { reason: String },
}

pub(crate) fn startup_role(
    request: Result<Option<HandoverRequest>, String>,
    own_generation_key: Option<&str>,
    now_unix_ms: u64,
    start_time_of: &dyn Fn(u32) -> Option<u64>,
) -> StartupRole {
    let request = match request {
        Ok(None) => return StartupRole::Ordinary,
        Ok(Some(request)) => request,
        Err(reason) => return StartupRole::Exit { reason },
    };
    if now_unix_ms >= request.expires_at_unix_ms {
        return StartupRole::Exit {
            reason: "handover request expired".to_string(),
        };
    }
    if own_generation_key != Some(request.generation_key.as_str()) {
        return StartupRole::Exit {
            reason: format!(
                "handover request is for generation {}, this daemon is {own_generation_key:?}",
                request.generation_key
            ),
        };
    }
    if start_time_of(request.predecessor_pid) != Some(request.predecessor_start_time) {
        return StartupRole::Exit {
            reason: format!(
                "handover predecessor pid {} is no longer the process that wrote the request",
                request.predecessor_pid
            ),
        };
    }
    StartupRole::Candidate {
        legacy_socket_path: request.legacy_socket_path,
        expires_at_unix_ms: request.expires_at_unix_ms,
    }
}

/// Time left before a candidate's handover request expires. Checked before every pre-commit
/// step; once it is gone the candidate aborts (AbortHandover) and exits 0 instead of committing,
/// and the predecessor keeps serving everything.
pub(crate) fn pre_commit_remaining(expires_at_unix_ms: u64, now_unix_ms: u64) -> Result<Duration, String> {
    if now_unix_ms >= expires_at_unix_ms {
        return Err(format!(
            "handover request expired before commit ({now_unix_ms} >= {expires_at_unix_ms})"
        ));
    }
    Ok(Duration::from_millis(expires_at_unix_ms - now_unix_ms))
}

/// A candidate whose request expired before commit exits 0 once the predecessor confirmed
/// AbortHandover; any other pre-commit failure, or an abort the predecessor did not confirm,
/// stays an error exit.
pub(crate) fn expired_candidate_exits_cleanly(expired_before_commit: bool, rolled_back: bool) -> bool {
    expired_before_commit && rolled_back
}

/// Reads the request in `runtime_dir`, removes it (it authorizes one start only) and decides the
/// startup role. Blocking.
pub(crate) fn take_startup_request(
    runtime_dir: &Path,
    own_generation_key: Option<&str>,
    now_unix_ms: u64,
    start_time_of: &dyn Fn(u32) -> Option<u64>,
) -> StartupRole {
    let read = read_record::<HandoverRequest>(&request_path(runtime_dir));
    if !matches!(read, Ok(None)) {
        remove_request(runtime_dir);
    }
    startup_role(read, own_generation_key, now_unix_ms, start_time_of)
}

/// Outcome of a candidate start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CandidateStart {
    /// The successor generation's job was asked to start (exactly one bootstrap/kickstart/start).
    Started,
    /// Nothing was started; the predecessor keeps serving.
    Refused(String),
}

/// The one OS-job operation a candidate start may issue.
pub(crate) trait CandidateStarter {
    fn start_candidate(&self, request: &HandoverRequest) -> Result<CandidateStart, String>;
}

/// The request on disk authorizes the caller only when it is exactly the caller's own request and
/// the caller is the predecessor it names.
pub(crate) fn authorizes(
    on_disk: &HandoverRequest,
    expected: &HandoverRequest,
    caller_pid: u32,
) -> Result<(), String> {
    if on_disk.nonce != expected.nonce {
        return Err("handover request nonce does not match".to_string());
    }
    if on_disk.generation_key != expected.generation_key {
        return Err("handover request generation_key does not match".to_string());
    }
    if on_disk.predecessor_pid != caller_pid
        || on_disk.predecessor_pid != expected.predecessor_pid
        || on_disk.predecessor_start_time != expected.predecessor_start_time
    {
        return Err("handover request was written by another predecessor".to_string());
    }
    if on_disk != expected {
        return Err("handover request differs from the caller's own".to_string());
    }
    Ok(())
}

/// `start_handover_candidate` core: under the admission lock, re-read the request and start the
/// successor job only when it authorizes the caller. Owner state is deliberately ignored: the
/// owner is the caller. Blocking.
pub(crate) fn start_handover_candidate_with(
    runtime_dir: &Path,
    admission_lock: &Path,
    expected: &HandoverRequest,
    caller_pid: u32,
    starter: &dyn CandidateStarter,
    lock_deadline: Duration,
) -> Result<CandidateStart, String> {
    let _admission = acquire_admission_lock(admission_lock, lock_deadline).map_err(|e| e.to_string())?;
    let Some(on_disk) = read_record::<HandoverRequest>(&request_path(runtime_dir))? else {
        return Ok(CandidateStart::Refused("no handover request on disk".to_string()));
    };
    if let Err(reason) = authorizes(&on_disk, expected, caller_pid) {
        return Ok(CandidateStart::Refused(reason));
    }
    starter.start_candidate(&on_disk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const KEY: &str = "2026.1013.2-0a1b2c3d";

    fn request(dir: &Path, pid: u32, start_time: u64, now: u64) -> HandoverRequest {
        HandoverRequest::new(
            dir.join("legacy-1.sock"),
            pid,
            start_time,
            7,
            KEY.to_string(),
            dir.join(format!("daemon-runtime/{KEY}/ferryx")),
            now,
        )
    }

    #[derive(Default)]
    struct CountingStarter {
        calls: RefCell<Vec<String>>,
    }

    impl CandidateStarter for CountingStarter {
        fn start_candidate(&self, request: &HandoverRequest) -> Result<CandidateStart, String> {
            self.calls.borrow_mut().push(request.generation_key.clone());
            Ok(CandidateStart::Started)
        }
    }

    #[test]
    fn handover_request_has_a_fresh_32_byte_nonce_and_a_30s_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let a = request(dir.path(), 1, 2, 1_000);
        let b = request(dir.path(), 1, 2, 1_000);
        assert_eq!(a.nonce.len(), 64);
        assert!(a.nonce.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.nonce, b.nonce);
        assert_eq!(a.expires_at_unix_ms, 31_000);
    }

    #[cfg(unix)]
    #[test]
    fn handover_request_is_written_mode_0600_and_round_trips() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let written = request(dir.path(), 11, 22, 0);
        write_request(dir.path(), &written).unwrap();
        let mode = std::fs::metadata(request_path(dir.path())).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let read: Option<HandoverRequest> = read_record(&request_path(dir.path())).unwrap();
        assert_eq!(read, Some(written));
    }

    #[test]
    fn staged_executable_must_be_inside_its_generation_dir_under_the_runtime_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("daemon-runtime");
        let exe = root.join(KEY).join("ferryx");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(validate_staged_executable(&root, &exe, KEY), Ok(()));
        assert!(validate_staged_executable(&root, &exe, "2026.1012.1-ffee0011").is_err());
        assert!(validate_staged_executable(&root, &dir.path().join("ferryx"), KEY).is_err());
        assert!(validate_staged_executable(&root, &root.join(KEY).join("../x/ferryx"), KEY).is_err());
        assert!(validate_staged_executable(&root, &root.join(KEY), KEY).is_err(), "the dir itself is not an executable inside it");
        assert!(validate_staged_executable(&root, &root.join(KEY).join("missing"), KEY).is_err());
        let bundle = root.join(KEY).join("Ferryx Daemon.app/Contents/MacOS/ferryx");
        assert_eq!(generation_key_under(&root, &bundle).as_deref(), Some(KEY));
    }

    #[test]
    fn startup_role_is_candidate_only_for_an_unexpired_own_generation_request_of_a_live_predecessor() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 4242, 99, 1_000);
        let alive = |pid: u32| (pid == 4242).then_some(99);
        assert_eq!(
            startup_role(Ok(Some(req.clone())), Some(KEY), 2_000, &alive),
            StartupRole::Candidate {
                legacy_socket_path: req.legacy_socket_path.clone(),
                expires_at_unix_ms: req.expires_at_unix_ms,
            }
        );
        assert_eq!(startup_role(Ok(None), Some(KEY), 2_000, &alive), StartupRole::Ordinary);
    }

    #[test]
    fn expiry_passing_mid_transfer_refuses_the_commit() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 4242, 99, 1_000);
        assert_eq!(
            pre_commit_remaining(req.expires_at_unix_ms, 1_000).unwrap(),
            Duration::from_millis(REQUEST_TTL_MS)
        );
        assert_eq!(
            pre_commit_remaining(req.expires_at_unix_ms, req.expires_at_unix_ms - 1).unwrap(),
            Duration::from_millis(1)
        );
        assert!(pre_commit_remaining(req.expires_at_unix_ms, req.expires_at_unix_ms).is_err());
        assert!(pre_commit_remaining(req.expires_at_unix_ms, u64::MAX).is_err());
        assert!(expired_candidate_exits_cleanly(true, true));
        assert!(!expired_candidate_exits_cleanly(true, false));
        assert!(!expired_candidate_exits_cleanly(false, true));
    }

    #[test]
    fn expired_request_exits_zero() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 4242, 99, 1_000);
        let alive = |pid: u32| (pid == 4242).then_some(99);
        assert!(matches!(
            startup_role(Ok(Some(req.clone())), Some(KEY), req.expires_at_unix_ms, &alive),
            StartupRole::Exit { .. }
        ));
    }

    #[test]
    fn wrong_generation_key_exits_zero() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 4242, 99, 1_000);
        let alive = |pid: u32| (pid == 4242).then_some(99);
        assert!(matches!(startup_role(Ok(Some(req.clone())), Some("other-00000000"), 2_000, &alive), StartupRole::Exit { .. }));
        assert!(matches!(startup_role(Ok(Some(req)), None, 2_000, &alive), StartupRole::Exit { .. }));
    }

    #[test]
    fn predecessor_identity_mismatch_exits_zero() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 4242, 99, 1_000);
        let reused_pid = |pid: u32| (pid == 4242).then_some(100);
        let dead = |_pid: u32| None;
        assert!(matches!(startup_role(Ok(Some(req.clone())), Some(KEY), 2_000, &reused_pid), StartupRole::Exit { .. }));
        assert!(matches!(startup_role(Ok(Some(req)), Some(KEY), 2_000, &dead), StartupRole::Exit { .. }));
        assert!(matches!(startup_role(Err("malformed".into()), Some(KEY), 2_000, &dead), StartupRole::Exit { .. }));
    }

    #[test]
    fn take_startup_request_consumes_the_file_in_every_case() {
        let dir = tempfile::tempdir().unwrap();
        let alive = |pid: u32| (pid == 4242).then_some(99);
        let req = request(dir.path(), 4242, 99, 1_000);
        write_request(dir.path(), &req).unwrap();
        assert!(matches!(take_startup_request(dir.path(), Some(KEY), 2_000, &alive), StartupRole::Candidate { .. }));
        assert!(!request_path(dir.path()).exists());
        write_request(dir.path(), &req).unwrap();
        assert!(matches!(take_startup_request(dir.path(), Some(KEY), u64::MAX, &alive), StartupRole::Exit { .. }));
        assert!(!request_path(dir.path()).exists(), "an invalid request is deleted");
        std::fs::write(request_path(dir.path()), b"not json").unwrap();
        assert!(matches!(take_startup_request(dir.path(), Some(KEY), 2_000, &alive), StartupRole::Exit { .. }));
        assert!(!request_path(dir.path()).exists(), "a malformed request is deleted");
        assert_eq!(take_startup_request(dir.path(), Some(KEY), 2_000, &alive), StartupRole::Ordinary);
    }

    #[test]
    fn start_handover_candidate_with_a_matching_request_starts_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 4242, 99, 1_000);
        write_request(dir.path(), &req).unwrap();
        let starter = CountingStarter::default();
        let lock = dir.path().join("admission.lock");
        let outcome = start_handover_candidate_with(dir.path(), &lock, &req, 4242, &starter, Duration::from_secs(1)).unwrap();
        assert_eq!(outcome, CandidateStart::Started);
        assert_eq!(starter.calls.borrow().len(), 1);
    }

    #[test]
    fn start_handover_candidate_with_a_mismatched_nonce_starts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 4242, 99, 1_000);
        write_request(dir.path(), &req).unwrap();
        let mut forged = req.clone();
        forged.nonce = "00".repeat(32);
        let starter = CountingStarter::default();
        let lock = dir.path().join("admission.lock");
        let outcome = start_handover_candidate_with(dir.path(), &lock, &forged, 4242, &starter, Duration::from_secs(1)).unwrap();
        assert!(matches!(outcome, CandidateStart::Refused(_)));
        assert!(starter.calls.borrow().is_empty());
    }

    #[test]
    fn start_handover_candidate_for_a_request_written_by_another_pid_starts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), 5151, 99, 1_000);
        write_request(dir.path(), &req).unwrap();
        let starter = CountingStarter::default();
        let lock = dir.path().join("admission.lock");
        let outcome = start_handover_candidate_with(dir.path(), &lock, &req, 4242, &starter, Duration::from_secs(1)).unwrap();
        assert!(matches!(outcome, CandidateStart::Refused(_)));
        assert!(starter.calls.borrow().is_empty());
        std::fs::remove_file(request_path(dir.path())).unwrap();
        let outcome = start_handover_candidate_with(dir.path(), &lock, &req, 5151, &starter, Duration::from_secs(1)).unwrap();
        assert!(matches!(outcome, CandidateStart::Refused(_)), "no request on disk authorizes nothing");
        assert!(starter.calls.borrow().is_empty());
    }
}
