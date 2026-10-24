// allow: SIZE_OK — daemon IPC server implementation with routing, session persistence offloading, remote control, and streaming
#[path = "machine_gateway.rs"]
mod machine_gateway;
// Server side of the frozen control-v1 channel (todo 15).
#[path = "control_server.rs"]
mod control_server;
use super::handover::HandoverManager;
use super::session_service::*;
use crate::daemon::agent_state::{AgentState, AgentStateHub, AgentStateSubscription};
use crate::daemon::protocol::AgentProviderSessionKey;
use crate::daemon::protocol::{
    AgentProviderSession, AgentStateReport, DaemonRemoteEvent, DaemonRemoteStatus, DaemonRequest,
    DaemonResponse, DaemonSessionDetails, DaemonStreamMessage, HistorySegmentWire, TerminalStartup,
    DAEMON_PROTOCOL_VERSION,
};
use crate::remote::auth::DevicePermission;
use crate::remote::server::{start_remote_server, RemoteServerHandle};
use crate::remote::state::{
    RemoteGatewayConfig, RemoteGatewayState, RemoteNetworkMode, REMOTE_GATEWAY_PORT,
};
use crate::session::{clear_session_from_path, load_session_from_path, save_session_to_path};
use crate::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use crate::worktree::{WorkspaceRegistry, WorktreeIdentity};
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
#[cfg(not(unix))]
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(all(test, unix))]
use tokio::net::UnixStream;
use tokio::sync::broadcast;

async fn rollback_transferred_readers<R, A, D>(
    relinquish: R,
    abort: impl FnOnce() -> A,
    decision: impl FnOnce() -> D,
) -> Result<(), String>
where
    R: std::future::Future<Output = Result<(), String>>,
    A: std::future::Future<Output = Result<DaemonResponse, String>>,
    D: std::future::Future<Output = Result<Option<super::handover_transaction::HandoverState>, String>>,
{
    relinquish.await?;
    match abort().await {
        Ok(DaemonResponse::AbortHandoverOk) => Ok(()),
        response => match decision().await? {
            Some(super::handover_transaction::HandoverState::Active) => Ok(()),
            _ => Err(format!("Abort decision is unconfirmed: {response:?}; predecessor must not resume")),
        },
    }
}

/// Installs the ownership record a predecessor exported for one session, after proving that the
/// record agrees with the predecessor's own describe answer.
///
/// The successor has never spawned this session, so its own registry can never be the authority
/// for it: the exported record is the only carrier of the workspace/worktree identity and of the
/// client-request ownership a transferred session keeps, and installing it is what makes this
/// daemon an authoritative owner *before* it takes the PTY master. A record that disagrees with the
/// predecessor's declaration, or is missing, refuses the session so the predecessor keeps it.
fn adopt_transferred_ownership(
    metadata: &RwLock<HashMap<String, StoredSessionMeta>>,
    session_id: &str,
    declared: &DaemonSessionDetails,
    exported: Option<serde_json::Value>,
) -> Result<(), String> {
    if declared.incarnation.as_deref().is_none_or(str::is_empty) {
        return Err(format!(
            "Predecessor declares no lifetime incarnation for '{session_id}'; source retained"
        ));
    }
    let Some(workspace_id) = declared.workspace_id.as_deref().filter(|id| !id.is_empty()) else {
        return Err(format!("Missing authoritative workspace owner of '{session_id}'"));
    };
    let Some(exported) = exported else {
        return Err(format!(
            "Predecessor exported no authoritative owner record for '{session_id}'; source retained"
        ));
    };
    let owner: StoredSessionMeta = serde_json::from_value(exported).map_err(|error| {
        format!("Predecessor exported an unreadable owner record for '{session_id}': {error}")
    })?;
    if owner.workspace_id != workspace_id || owner.worktree != declared.worktree {
        return Err(format!(
            "Export workspace domain does not match the predecessor's declaration for '{session_id}'; source retained"
        ));
    }
    metadata.write().insert(session_id.to_owned(), owner);
    Ok(())
}

/// Rolls back a handover whose CommitHandover did not land (the predecessor recorded no
/// retirement): the adopted sessions are relinquished first, then the predecessor is told to
/// abort. The abort is what resumes the readers it paused for the export; without it the
/// predecessor keeps serving every session with its output frozen.
async fn release_after_unconfirmed_commit<R, A>(
    relinquish: R,
    abort: impl FnOnce() -> A,
) -> Result<(), String>
where
    R: std::future::Future<Output = Result<(), String>>,
    A: std::future::Future<Output = Result<DaemonResponse, String>>,
{
    relinquish.await?;
    match abort().await {
        Ok(DaemonResponse::AbortHandoverOk) => {}
        response => tracing::warn!(
            ?response,
            "Predecessor did not confirm AbortHandover after the failed commit"
        ),
    }
    Ok(())
}

/// Drops the ownership records a successor installed for sessions it handed back to the
/// predecessor: a relinquished session must not leave this daemon claiming its workspace.
fn release_adopted_ownership(
    metadata: &RwLock<HashMap<String, StoredSessionMeta>>,
    session_ids: &[String],
) {
    let mut records = metadata.write();
    for session_id in session_ids {
        records.remove(session_id);
    }
}

#[cfg(test)]
mod pane_reader_rollback_tests {
    use super::*;

    #[tokio::test]
    async fn pane_liveness_reader_rollback_relinquishes_before_predecessor_resume() {
        let (release, released) = tokio::sync::oneshot::channel();
        let (entered, started) = tokio::sync::oneshot::channel();
        let relinquished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_state = relinquished.clone();
        let abort_state = relinquished.clone();
        let rollback = rollback_transferred_readers(async move {
            entered.send(()).unwrap();
            released.await.unwrap();
            reader_state.store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        }, move || async move {
            assert!(abort_state.load(std::sync::atomic::Ordering::Acquire));
            Ok(DaemonResponse::AbortHandoverOk)
        }, || async { Ok(None) });
        let trigger = async move { started.await.unwrap(); release.send(()).unwrap(); };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(rollback, trigger)
        }).await.unwrap();
        result.unwrap();
        assert!(relinquished.load(std::sync::atomic::Ordering::Acquire));
    }

    #[tokio::test]
    async fn failed_commit_rollback_tells_the_predecessor_to_resume_after_relinquishing() {
        let steps = std::sync::Mutex::new(Vec::new());
        release_after_unconfirmed_commit(
            async {
                steps.lock().unwrap().push("relinquish");
                Ok(())
            },
            || async {
                steps.lock().unwrap().push("abort");
                Ok(DaemonResponse::AbortHandoverOk)
            },
        )
        .await
        .unwrap();
        assert_eq!(*steps.lock().unwrap(), ["relinquish", "abort"]);
    }

    #[tokio::test]
    async fn failed_commit_rollback_keeps_the_predecessor_frozen_when_relinquish_fails() {
        let result = release_after_unconfirmed_commit(
            async { Err("reader still owns handle".into()) },
            || async { panic!("predecessor must remain frozen") },
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn pane_liveness_reader_rollback_failed_relinquish_never_sends_abort() {
        let result = rollback_transferred_readers(async { Err("reader still owns handle".into()) },
            || async { panic!("predecessor must remain frozen") }, || async { Ok(None) }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn pane_liveness_reader_rollback_ambiguous_abort_never_confirms_resume() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("undecided.sock");
        let result = rollback_transferred_readers(async { Ok(()) },
            || async { Err("abort delivery ambiguous".into()) },
            || async { HandoverManager::recorded_decision(&path) }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn pane_liveness_reader_rollback_decided_ambiguous_reads_recorded_abort() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("legacy.sock");
        let identity = path.to_string_lossy().into_owned();
        let mut transaction = super::super::handover_transaction::HandoverTransaction::new_simple(
            &identity, &identity, 1, 1, 2, 2, &identity);
        transaction.rollback_reason = Some("abort".into());
        std::fs::write(path.with_extension("transaction"), serde_json::to_vec(&transaction).unwrap()).unwrap();
        rollback_transferred_readers(async { Ok(()) }, || async { Err("lost ACK".into()) },
            || async { HandoverManager::recorded_decision(&path) }).await.unwrap();
    }
}

#[cfg(test)]
mod pane_liveness_adopted_ownership_tests {
    use super::*;

    fn worktree(slug: &str) -> WorktreeIdentity {
        WorktreeIdentity {
            ws_id: "ws-declared".into(),
            slug: slug.into(),
        }
    }

    fn owner_record(workspace_id: &str, worktree: Option<WorktreeIdentity>) -> StoredSessionMeta {
        StoredSessionMeta {
            machine_session: None,
            client_request_id: format!("req-{workspace_id}"),
            workspace_id: workspace_id.to_owned(),
            worktree: worktree.clone(),
            cwd: PathBuf::from("/repo"),
            provider_claim: None,
            spawn_fingerprint: SpawnRequestFingerprint {
                workspace_id: workspace_id.to_owned(),
                worktree,
                cwd: None,
                cols: 80,
                rows: 24,
                shell: None,
                provider_claim: None,
                startup: None,
                requested_session_id: None,
            },
        }
    }

    fn declared(
        workspace_id: Option<&str>,
        worktree: Option<WorktreeIdentity>,
        incarnation: Option<&str>,
    ) -> DaemonSessionDetails {
        let mut details = DaemonSessionDetails::new(
            "session-owned".into(),
            workspace_id.map(str::to_owned),
            worktree,
            Some("/repo".into()),
            80,
            24,
            true,
            None,
            None,
            None,
            false,
        );
        details.incarnation = incarnation.map(str::to_owned);
        details
    }

    fn metadata() -> RwLock<HashMap<String, StoredSessionMeta>> {
        RwLock::new(HashMap::new())
    }

    /// The successor has no record of its own for a session it never spawned, so the record the
    /// predecessor exports is the one that must end up owning the session here.
    #[test]
    fn pane_liveness_adoption_installs_the_exported_owner_record() {
        let metadata = metadata();
        let record = owner_record("ws-declared", None);
        adopt_transferred_ownership(
            &metadata,
            "session-owned",
            &declared(Some("ws-declared"), None, Some("incarnation-a")),
            Some(serde_json::to_value(&record).unwrap()),
        )
        .expect("an exported record that matches the declaration is adopted");
        let installed = metadata
            .read()
            .get("session-owned")
            .cloned()
            .expect("the adopted session must have an authoritative workspace owner");
        assert_eq!(installed.workspace_id, "ws-declared");
        assert_eq!(installed.client_request_id, "req-ws-declared");
    }

    #[test]
    fn pane_liveness_adoption_refuses_a_record_from_another_workspace() {
        let metadata = metadata();
        let result = adopt_transferred_ownership(
            &metadata,
            "session-owned",
            &declared(Some("ws-declared"), None, Some("incarnation-a")),
            Some(serde_json::to_value(owner_record("ws-other", None)).unwrap()),
        );
        assert!(result.is_err(), "an export outside the declared workspace is refused");
        assert!(
            metadata.read().is_empty(),
            "a refused session must not leave a workspace owner behind"
        );
    }

    #[test]
    fn pane_liveness_adoption_refuses_a_worktree_domain_mismatch() {
        let metadata = metadata();
        let result = adopt_transferred_ownership(
            &metadata,
            "session-owned",
            &declared(
                Some("ws-declared"),
                Some(worktree("main")),
                Some("incarnation-a"),
            ),
            Some(
                serde_json::to_value(owner_record("ws-declared", Some(worktree("other"))))
                    .unwrap(),
            ),
        );
        assert!(result.is_err(), "a different worktree is a different identity domain");
        assert!(metadata.read().is_empty());
    }

    #[test]
    fn pane_liveness_adoption_refuses_a_missing_owner_record() {
        let metadata = metadata();
        let result = adopt_transferred_ownership(
            &metadata,
            "session-owned",
            &declared(Some("ws-declared"), None, Some("incarnation-a")),
            None,
        );
        assert!(result.is_err(), "a session with no exported record cannot be owned");
        assert!(metadata.read().is_empty());
    }

    #[test]
    fn pane_liveness_adoption_refuses_a_predecessor_without_an_incarnation() {
        let metadata = metadata();
        let result = adopt_transferred_ownership(
            &metadata,
            "session-owned",
            &declared(Some("ws-declared"), None, None),
            Some(serde_json::to_value(owner_record("ws-declared", None)).unwrap()),
        );
        assert!(result.is_err(), "an unprovable incarnation is not an identity domain");
        assert!(metadata.read().is_empty());
    }

    #[test]
    fn pane_liveness_adoption_refuses_a_declaration_without_a_workspace_owner() {
        let metadata = metadata();
        let result = adopt_transferred_ownership(
            &metadata,
            "session-owned",
            &declared(None, None, Some("incarnation-a")),
            Some(serde_json::to_value(owner_record("ws-declared", None)).unwrap()),
        );
        assert!(
            result.is_err(),
            "a session with no declared workspace owner cannot be adopted"
        );
        assert!(metadata.read().is_empty());
    }

    /// Relinquishing a session hands it back to the predecessor; keeping the installed record would
    /// leave this daemon claiming a workspace it does not own.
    #[test]
    fn pane_liveness_relinquished_sessions_lose_their_installed_ownership() {
        let metadata = metadata();
        for session in ["session-a", "session-b"] {
            adopt_transferred_ownership(
                &metadata,
                session,
                &declared(Some("ws-declared"), None, Some("incarnation-a")),
                Some(serde_json::to_value(owner_record("ws-declared", None)).unwrap()),
            )
            .unwrap();
        }
        assert_eq!(metadata.read().len(), 2);
        release_adopted_ownership(&metadata, &["session-a".to_string()]);
        let remaining = metadata.read();
        assert!(!remaining.contains_key("session-a"));
        assert!(remaining.contains_key("session-b"));
    }
}

/// Returns the compile-time development profile. Debug builds isolate session state and daemon
/// endpoints so a release GUI cannot attach to a dev daemon; release paths remain byte-identical.
pub(crate) fn is_dev_runtime() -> bool {
    cfg!(debug_assertions)
}

/// Returns the QA and multi-instance session directory override. See
/// `docs/SESSION_STATE_WIPE_2026-08-31.md`; release defaults intentionally remain byte-identical.
pub(crate) fn session_dir_override() -> Option<PathBuf> {
    std::env::var_os("FERRYX_SESSION_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

#[cfg(feature = "local-split-qa")]
fn transport_qa_isolated_profile(runtime_dir: &Path, data_dir: Option<&std::ffi::OsStr>) -> bool {
    let Some(data_dir) = data_dir.filter(|dir| !dir.is_empty()) else {
        return false;
    };
    let data_dir = Path::new(data_dir);
    let Ok(runtime_dir) = runtime_dir.canonicalize() else {
        return false;
    };
    let Ok(data_dir) = data_dir.canonicalize() else {
        return false;
    };
    runtime_dir != Path::new("/") && data_dir != Path::new("/")
        && runtime_dir != data_dir && !runtime_dir.starts_with(&data_dir) && !data_dir.starts_with(&runtime_dir)
}

#[cfg(feature = "local-split-qa")]
fn active_transport_qa_profile() -> bool {
    transport_qa_isolated_profile(&get_runtime_dir(), std::env::var_os("FERRYX_DATA_DIR").as_deref())
}

#[cfg(all(test, feature = "local-split-qa"))]
#[test]
fn transport_qa_profile_refuses_missing_and_shared_profile_paths() {
    use std::ffi::OsStr;
    use std::path::Path;

    let root = tempfile::tempdir().expect("isolated guard fixture");
    let profile = root.path().join("profile");
    let runtime = profile.join("runtime");
    let sibling = root.path().join("other");
    std::fs::create_dir_all(&runtime).expect("profile runtime");
    std::fs::create_dir_all(&sibling).expect("sibling data dir");

    assert!(!transport_qa_isolated_profile(&runtime, None));
    assert!(!transport_qa_isolated_profile(&runtime, Some(profile.as_os_str())));
    assert!(!transport_qa_isolated_profile(&profile, Some(profile.as_os_str())));
    assert!(transport_qa_isolated_profile(&runtime, Some(sibling.as_os_str())));
    assert!(!transport_qa_isolated_profile(Path::new("/qa/runtime"), Some(OsStr::new("/"))));
}

pub(crate) fn resolve_session_path(
    env_dir: Option<&Path>,
    is_dev: bool,
    release_path: &Path,
    dev_path: &Path,
) -> PathBuf {
    env_dir
        .map(|dir| dir.join("session_state.json"))
        .unwrap_or_else(|| {
            if is_dev {
                dev_path.to_path_buf()
            } else {
                release_path.to_path_buf()
            }
        })
}

#[cfg(test)]
mod session_path_tests {
    use super::resolve_session_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn session_path_override_wins_for_release() {
        assert_eq!(
            resolve_session_path(
                Some(Path::new("/qa/session")),
                false,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/qa/session/session_state.json")
        );
    }

    #[test]
    fn session_path_dev_uses_isolated_path_without_override() {
        assert_eq!(
            resolve_session_path(
                None,
                true,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/app/rorca-dev/session_state.json")
        );
    }

    #[test]
    fn session_path_release_preserves_existing_path_without_override() {
        assert_eq!(
            resolve_session_path(
                None,
                false,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/app/rorca/session_state.json")
        );
    }

    #[test]
    fn session_path_override_wins_for_dev() {
        assert_eq!(
            resolve_session_path(
                Some(Path::new("/qa/session")),
                true,
                Path::new("/app/rorca/session_state.json"),
                Path::new("/app/rorca-dev/session_state.json"),
            ),
            PathBuf::from("/qa/session/session_state.json")
        );
    }
}

/// Returns the daemon runtime directory (`/tmp/ferryx-{uid}`). Debug builds use a separate
/// directory to prevent dev and release daemons from sharing endpoints.
#[cfg(unix)]
pub fn get_runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `libc::getuid` is a stateless, side-effect-free POSIX syscall wrapper that
    // takes no arguments, dereferences no pointers, and always returns the current process's UID.
    let uid = unsafe { libc::getuid() };
    let suffix = if is_dev_runtime() { "-dev" } else { "" };
    PathBuf::from(format!("/tmp/ferryx-{uid}{suffix}"))
}

/// The runtime directory used before the rename (`/tmp/rorca-{uid}`). For one release the daemon
/// also listens on `daemon.sock` here, so a GUI from before the rename still connects, and a new
/// client still finds a daemon from before the rename. Under a `FERRYX_RUNTIME_DIR` override
/// there is no legacy directory unless `FERRYX_LEGACY_RUNTIME_DIR` names one (isolated tests/QA).
#[cfg(unix)]
pub fn get_legacy_runtime_dir() -> Option<PathBuf> {
    if std::env::var_os("FERRYX_RUNTIME_DIR").is_some() {
        return std::env::var_os("FERRYX_LEGACY_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from);
    }
    // SAFETY: `getuid` takes no arguments and returns the current process UID.
    let uid = unsafe { libc::getuid() };
    let suffix = if is_dev_runtime() { "-dev" } else { "" };
    Some(PathBuf::from(format!("/tmp/rorca-{uid}{suffix}")))
}

#[cfg(not(unix))]
pub fn get_legacy_runtime_dir() -> Option<PathBuf> {
    None
}

/// `daemon.sock` in [`get_legacy_runtime_dir`].
pub fn get_legacy_socket_path() -> Option<PathBuf> {
    get_legacy_runtime_dir().map(|dir| dir.join("daemon.sock"))
}

/// True when a daemon from before the rename still holds the legacy directory's `daemon.lock`.
#[cfg(unix)]
fn legacy_lock_held_elsewhere(legacy_dir: &Path) -> bool {
    crate::daemon::handover_socket::lock_is_held_elsewhere(&legacy_dir.join("daemon.lock"))
        .unwrap_or(false)
}

/// The legacy socket, when nothing serves the canonical socket and a daemon from before the
/// rename owns the legacy one (it holds the legacy `daemon.lock`). Blocking.
#[cfg(unix)]
pub fn legacy_owner_socket_path() -> Option<PathBuf> {
    if fs::symlink_metadata(get_socket_path()).is_ok() {
        return None;
    }
    let legacy = get_legacy_socket_path()?;
    let dir = legacy.parent()?;
    (legacy_lock_held_elsewhere(dir) && fs::symlink_metadata(&legacy).is_ok()).then_some(legacy)
}

#[cfg(not(unix))]
pub fn legacy_owner_socket_path() -> Option<PathBuf> {
    None
}

/// The legacy runtime dir's `daemon.lock`, held while this daemon serves the legacy socket so a
/// launcher from before the rename sees an owner instead of starting a second daemon.
#[cfg(unix)]
static LEGACY_COMPAT_LOCK: Mutex<Option<DaemonLockFile>> = parking_lot::const_mutex(None);

/// Takes the legacy `daemon.lock`. Without `wait` a held lock is Ok(None); with `wait` the call
/// blocks until a daemon from before the rename that still owns it exits.
#[cfg(unix)]
fn take_legacy_lock(path: &Path, wait: bool) -> Result<Option<DaemonLockFile>, String> {
    use std::os::unix::io::AsRawFd;
    let file = open_secure_lock_file(path)?;
    if !wait {
        return Ok(DaemonLockFile::try_lock(file).ok());
    }
    // SAFETY: `file` stays open for the whole call; LOCK_EX blocks until the lock is granted.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(format!(
            "Failed to lock {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(Some(DaemonLockFile::from_locked_file(file)))
}

#[cfg(not(unix))]
pub fn get_runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("TEMP"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Ferryx")
        .join(if is_dev_runtime() {
            "runtime-dev"
        } else {
            "runtime"
        })
}

#[cfg(unix)]
pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.sock")
}

#[cfg(not(unix))]
pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.port")
}

/// Uncertain-write fixture (QA builds only): after a remote write reached the remote PTY,
/// park before WriteOk while QaWriteAckHold is armed for the session. The park ends on
/// release (returns true: send WriteOk) or when the connection's reader reaches EOF/error
/// (returns false: the client is gone and the WriteOk is dropped), so a disconnected client
/// never leaves the connection task parked. The EOF probe only fills the buffer and never
/// consumes bytes; a pipelined request keeps the park waiting on release alone.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
async fn qa_hold_write_ack<R: tokio::io::AsyncBufRead + Unpin>(session_id: &str, reader: &mut R) -> bool {
    let Some(release) = crate::ipc::qa_barrier::write_ack_hold().applied(session_id) else {
        return true;
    };
    let closed = async {
        let eof = std::future::poll_fn(|cx| {
            std::pin::Pin::new(&mut *reader)
                .poll_fill_buf(cx)
                .map(|filled| filled.map_or(true, |buf| buf.is_empty()))
        })
        .await;
        if !eof {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = release.notified() => true,
        _ = closed => false,
    }
}

#[cfg(not(all(feature = "local-split-qa", feature = "native-terminal")))]
async fn qa_hold_write_ack<R>(_session_id: &str, _reader: &mut R) -> bool {
    true
}

#[cfg(all(test, feature = "local-split-qa", feature = "native-terminal"))]
mod qa_write_ack_hold_close_tests {
    use std::time::Duration;

    fn connection() -> (tokio::io::DuplexStream, tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>, tokio::io::WriteHalf<tokio::io::DuplexStream>) {
        let (client, server) = tokio::io::duplex(64);
        let (read_half, write_half) = tokio::io::split(server);
        (client, tokio::io::BufReader::new(read_half), write_half)
    }

    #[tokio::test]
    async fn held_ack_is_dropped_when_connection_closes() {
        let session = "qa-hold-close-on-disconnect";
        let hold = crate::ipc::qa_barrier::write_ack_hold();
        hold.arm(session);
        let mut applied = hold.subscribe_applied();
        let (client, mut reader, _write_half) = connection();
        let parked = tokio::spawn(async move { super::qa_hold_write_ack(session, &mut reader).await });
        // The write-applied event proves the task reached the park.
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), applied.recv()).await.unwrap().unwrap();
            if event.session_id == session { break; }
        }
        assert!(!parked.is_finished(), "nothing released the hold and the client is still connected");
        drop(client);
        let deliver = tokio::time::timeout(Duration::from_secs(5), parked).await
            .expect("connection close must end the park").unwrap();
        assert!(!deliver, "WriteOk must be dropped for a closed connection");
        assert!(hold.release(session), "close does not disarm; explicit release still does");
    }

    #[tokio::test]
    async fn held_ack_is_delivered_on_release_while_connected() {
        let session = "qa-hold-release-connected";
        let hold = crate::ipc::qa_barrier::write_ack_hold();
        hold.arm(session);
        let mut applied = hold.subscribe_applied();
        let (_client, mut reader, _write_half) = connection();
        let parked = tokio::spawn(async move { super::qa_hold_write_ack(session, &mut reader).await });
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), applied.recv()).await.unwrap().unwrap();
            if event.session_id == session { break; }
        }
        assert!(hold.release(session));
        let deliver = tokio::time::timeout(Duration::from_secs(5), parked).await.unwrap().unwrap();
        assert!(deliver);
    }
}

pub fn get_lock_path() -> PathBuf {
    get_runtime_dir().join("daemon.lock")
}

pub fn get_file_mtime_ms(path: &Path) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as u64)
}

pub fn resolve_binary_identity_with<FExe, FMtime>(
    get_exe: FExe,
    get_mtime: FMtime,
) -> (Option<String>, Option<u64>)
where
    FExe: Fn() -> std::io::Result<PathBuf>,
    FMtime: Fn(&Path) -> Option<u64>,
{
    match get_exe() {
        Ok(exe_path) => {
            let mtime = get_mtime(&exe_path);
            (Some(exe_path.to_string_lossy().into_owned()), mtime)
        }
        Err(error) => {
            tracing::warn!("Failed to determine current_exe for binary identity: {error}");
            (None, None)
        }
    }
}

pub fn resolve_binary_identity() -> (Option<String>, Option<u64>) {
    resolve_binary_identity_with(std::env::current_exe, get_file_mtime_ms)
}

/// Resolve the executable a daemon upgrade or handover should launch.
///
/// A daemon that survived an in-place application replacement keeps executing from an
/// unlinked inode, so `current_exe()` can name a path that no longer exists. Launching that
/// path fails, and it fails *after* the canonical listener has already been handed over,
/// which would leave no daemon serving either socket. Only ever resolve to a file that
/// exists right now; the caller must treat `None` as "upgrade unavailable".
pub fn resolve_upgrade_target_exe_with<FExe>(
    explicit: Option<&str>,
    get_exe: FExe,
) -> Option<PathBuf>
where
    FExe: Fn() -> std::io::Result<PathBuf>,
{
    if let Some(path) = explicit.map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    match get_exe() {
        Ok(exe) if exe.is_file() => Some(exe),
        _ => None,
    }
}

pub fn resolve_upgrade_target_exe(explicit: Option<&str>) -> Option<PathBuf> {
    resolve_upgrade_target_exe_with(explicit, std::env::current_exe)
}

#[cfg(unix)]
pub fn clear_cloexec(fd: std::os::unix::io::RawFd) -> Result<(), std::io::Error> {
    // SAFETY: Foreign Function Interface to fcntl.
    // Invariant: fd is a valid open file descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return Err(std::io::Error::last_os_error());
        }
        if (flags & libc::FD_CLOEXEC) != 0 {
            if libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn perform_daemon_exec_with_path(exe: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::process::CommandExt;

    tracing::info!("Re-executing daemon with binary at: {}", exe.display());

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    // The re-executed image keeps this pid, so it is still the unit's main process and reports
    // `READY=1` again at readiness.
    crate::daemon::supervisor_linux::forward_notify_socket(&mut cmd);
    let err = cmd.exec();
    tracing::error!("Daemon exec failed: {err}");
    Err(err)
}

#[cfg(unix)]
pub fn perform_daemon_exec() -> Result<(), std::io::Error> {
    let exe = std::env::current_exe()?;
    perform_daemon_exec_with_path(&exe)
}

pub(crate) fn get_persistent_lock_path() -> Option<PathBuf> {
    std::env::var_os("FERRYX_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(unix)]
            {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferryx"))
            }
            #[cfg(windows)]
            {
                std::env::var_os("APPDATA")
                    .map(|appdata| PathBuf::from(appdata).join("Ferryx"))
                    .or_else(|| {
                        std::env::var_os("USERPROFILE")
                            .map(|home| PathBuf::from(home).join(".ferryx"))
                    })
            }
            #[cfg(not(any(unix, windows)))]
            {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferryx"))
            }
        })
        .map(|base| {
            base.join("locks").join(if is_dev_runtime() {
                "daemon-dev.lock"
            } else {
                "daemon.lock"
            })
        })
}

pub fn get_default_session_path() -> PathBuf {
    let override_dir = session_dir_override();
    let is_dev = is_dev_runtime();
    if override_dir.is_some() {
        let path = resolve_session_path(
            override_dir.as_deref(),
            is_dev,
            Path::new(""),
            Path::new(""),
        );
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        return path;
    }

    let path = if let Some(base) = dirs_next().or_else(dirs_fallback) {
        resolve_session_path(
            override_dir.as_deref(),
            is_dev,
            &base.join("rorca").join("session_state.json"),
            &base.join("rorca-dev").join("session_state.json"),
        )
    } else {
        let runtime_dir = get_runtime_dir();
        resolve_session_path(
            override_dir.as_deref(),
            is_dev,
            &runtime_dir.join("session_state.json"),
            &runtime_dir.join("session_state.dev.json"),
        )
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    path
}

fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

/// Root of the immutable generation-keyed daemon runtime copies (`daemon::runtime_stage`).
/// `FERRYX_DATA_DIR` isolates QA. Windows uses `%LOCALAPPDATA%` like `get_runtime_dir`, not
/// `dirs_next` (APPDATA there), because roaming profiles must not carry executables.
pub fn daemon_runtime_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let platform_base = std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Ferryx"));
    #[cfg(target_os = "macos")]
    let platform_base = dirs_next().map(|dir| dir.join("Ferryx"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let platform_base = dirs_next().map(|dir| dir.join("ferryx"));
    super::runtime_stage::runtime_root_from(std::env::var_os("FERRYX_DATA_DIR").as_deref(), platform_base)
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".rorca"))
}

#[derive(Clone, Copy)]
enum RuntimeNodeKind {
    Directory,
    RegularFile,
    Socket,
}

#[cfg(unix)]
fn validate_safe_ownership_and_type_for_uid(
    path: &Path,
    kind: RuntimeNodeKind,
    expected_uid: libc::uid_t,
) -> Result<(), String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Path {} is a symlink, which is prohibited for daemon runtime",
            path.display()
        ));
    }
    if meta.uid() != expected_uid {
        return Err(format!(
            "Path {} is owned by UID {} (expected current UID {})",
            path.display(),
            meta.uid(),
            expected_uid
        ));
    }
    let valid_type = match kind {
        RuntimeNodeKind::Directory => meta.file_type().is_dir(),
        RuntimeNodeKind::RegularFile => meta.file_type().is_file(),
        RuntimeNodeKind::Socket => meta.file_type().is_socket(),
    };
    if !valid_type {
        return Err(format!(
            "Path {} has an invalid runtime node type",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_safe_ownership_and_type_for_uid(
    path: &Path,
    kind: RuntimeNodeKind,
    _expected_uid: u32,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Path {} is a symlink, which is prohibited for daemon runtime",
            path.display()
        ));
    }
    let valid_type = match kind {
        RuntimeNodeKind::Directory => meta.file_type().is_dir(),
        RuntimeNodeKind::RegularFile | RuntimeNodeKind::Socket => !meta.file_type().is_dir(),
    };
    if !valid_type {
        return Err(format!(
            "Path {} has an invalid runtime node type",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_safe_ownership_and_type(path: &Path, kind: RuntimeNodeKind) -> Result<(), String> {
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `libc::getuid` is a stateless, side-effect-free POSIX syscall wrapper that
    // takes no arguments, dereferences no pointers, and always returns the current process's UID.
    validate_safe_ownership_and_type_for_uid(path, kind, unsafe { libc::getuid() })
}

#[cfg(not(unix))]
fn validate_safe_ownership_and_type(path: &Path, kind: RuntimeNodeKind) -> Result<(), String> {
    validate_safe_ownership_and_type_for_uid(path, kind, 0)
}

#[cfg(unix)]
pub(crate) fn validate_runtime_socket_path(path: &Path) -> Result<(), String> {
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `libc::getuid` is a stateless, side-effect-free POSIX syscall wrapper that
    // takes no arguments, dereferences no pointers, and always returns the current process's UID.
    validate_runtime_socket_path_for_uid(path, unsafe { libc::getuid() })
}

#[cfg(not(unix))]
pub(crate) fn validate_runtime_socket_path(path: &Path) -> Result<(), String> {
    validate_runtime_socket_path_for_uid(path, 0)
}

#[cfg(unix)]
pub(crate) fn validate_runtime_socket_path_for_uid(
    path: &Path,
    expected_uid: libc::uid_t,
) -> Result<(), String> {
    let runtime_dir = path
        .parent()
        .ok_or_else(|| format!("Daemon socket {} has no runtime directory", path.display()))?;
    validate_safe_ownership_and_type_for_uid(
        runtime_dir,
        RuntimeNodeKind::Directory,
        expected_uid,
    )?;
    let mode = fs::symlink_metadata(runtime_dir)
        .map_err(|error| format!("Failed to verify {}: {error}", runtime_dir.display()))?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o700 {
        return Err(format!(
            "Daemon runtime directory {} has mode {mode:o}, expected 700",
            runtime_dir.display()
        ));
    }
    validate_safe_ownership_and_type_for_uid(path, RuntimeNodeKind::Socket, expected_uid)
}

#[cfg(not(unix))]
pub(crate) fn validate_runtime_socket_path_for_uid(
    path: &Path,
    _expected_uid: u32,
) -> Result<(), String> {
    if let Some(runtime_dir) = path.parent() {
        validate_safe_ownership_and_type_for_uid(runtime_dir, RuntimeNodeKind::Directory, 0)?;
    }
    validate_safe_ownership_and_type_for_uid(path, RuntimeNodeKind::RegularFile, 0)?;
    Ok(())
}

pub(crate) fn ensure_runtime_directory(path: &Path) -> Result<(), String> {
    match fs::create_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "Failed to create daemon runtime directory {}: {error}",
                path.display()
            ));
        }
    }
    validate_safe_ownership_and_type(path, RuntimeNodeKind::Directory)?;
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "Failed to secure daemon runtime directory {}: {error}",
                path.display()
            )
        })?;
        let mode = fs::symlink_metadata(path)
            .map_err(|error| format!("Failed to verify {}: {error}", path.display()))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            return Err(format!(
                "Daemon runtime directory {} has mode {mode:o}, expected 700",
                path.display()
            ));
        }
    }
    Ok(())
}

pub(crate) fn open_secure_lock_file(path: &Path) -> Result<File, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_safe_ownership_and_type(path, RuntimeNodeKind::RegularFile)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect lock file: {error}")),
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);

    #[cfg(unix)]
    {
        options.mode(0o600);
        options.custom_flags(libc::O_NOFOLLOW);
    }

    let file = options
        .open(path)
        .map_err(|error| format!("Failed to open lock file {}: {error}", path.display()))?;
    validate_safe_ownership_and_type(path, RuntimeNodeKind::RegularFile)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure lock file: {error}"))?;
    Ok(file)
}

#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct DaemonLockFile {
    _file: File,
}

#[cfg(unix)]
impl DaemonLockFile {
    pub(crate) fn try_lock(file: File) -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid File Descriptor.
        // Invariant: `file.as_raw_fd()` returns a valid open file descriptor borrowed from `file`,
        // which remains open and valid for the duration of the `libc::flock` call.
        let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if ret != 0 {
            return Err("Another daemon instance is already holding the lock.".into());
        }
        Ok(Self { _file: file })
    }

    /// Detaches the underlying file without invoking `LOCK_UN`, preserving the lock
    /// across ownership transfer to the successor process.
    pub(crate) fn detach_without_unlock(mut self) -> File {
        // SAFETY: `self._file` is a valid File. We read it out and forget `self`
        // so that the `Drop` implementation (which calls `LOCK_UN`) does not execute.
        let file = unsafe { std::ptr::read(&self._file) };
        std::mem::forget(self);
        file
    }

    /// Constructs a `DaemonLockFile` from an already-locked file descriptor received
    /// during handover transfer.
    pub(crate) fn from_locked_file(file: File) -> Self {
        Self { _file: file }
    }
}

#[cfg(unix)]
impl Drop for DaemonLockFile {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid File Descriptor.
        // Invariant: `self._file.as_raw_fd()` returns a valid open file descriptor owned by `self._file`,
        // which has not been closed yet. Calling `libc::flock` with `LOCK_UN` synchronously clears
        // the exclusive lock before the descriptor is closed by `_file`'s drop.
        unsafe {
            libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct DaemonLockFile {
    file: File,
}

#[cfg(windows)]
impl DaemonLockFile {
    pub(crate) fn try_lock(file: File) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Storage::FileSystem::{
            LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = file.as_raw_handle() as HANDLE;
        // SAFETY:
        // Category: Uninitialized Memory.
        // Invariant: `OVERLAPPED` is a C-compatible repr(C) struct whose all-zero bit pattern
        // is valid memory representing zero offset (Offset=0, OffsetHigh=0) and null hEvent.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid Handle Dereference.
        // Invariant: `handle` is guaranteed to be a valid, open Win32 file handle owned by `file`,
        // which remains open and valid for the duration of this Win32 `LockFileEx` call.
        // `&mut overlapped` points to a valid, properly aligned, stack-allocated `OVERLAPPED` struct.
        let ret = unsafe {
            LockFileEx(
                handle,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            )
        };

        if ret == 0 {
            return Err("Another daemon instance is already holding the lock.".into());
        }

        Ok(Self { file })
    }

    /// Detaches the underlying file without invoking `UnlockFileEx`, preserving the lock
    /// across ownership transfer to the successor process.
    pub(crate) fn detach_without_unlock(self) -> File {
        let file = unsafe { std::ptr::read(&self.file) };
        std::mem::forget(self);
        file
    }

    /// Constructs a `DaemonLockFile` from an already-locked file handle received
    /// during handover transfer.
    pub(crate) fn from_locked_file(file: File) -> Self {
        Self { file }
    }
}

#[cfg(windows)]
impl Drop for DaemonLockFile {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = self.file.as_raw_handle() as HANDLE;
        // SAFETY:
        // Category: Uninitialized Memory.
        // Invariant: `OVERLAPPED` is a C-compatible repr(C) struct whose all-zero bit pattern
        // is valid memory representing zero offset (Offset=0, OffsetHigh=0) and null hEvent.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid Handle Dereference.
        // Invariant: `handle` is guaranteed to be a valid, open Win32 file handle owned by `self.file`,
        // which has not been closed yet. The offset (0) and length (1 byte) exactly match the
        // exclusive range locked in `DaemonLockFile::try_lock`.
        unsafe {
            UnlockFileEx(handle, 0, 1, 0, &mut overlapped);
        }
    }
}

#[cfg(not(any(unix, windows)))]
#[derive(Debug)]
pub(crate) struct DaemonLockFile {
    _file: File,
}

#[cfg(not(any(unix, windows)))]
impl DaemonLockFile {
    pub(crate) fn try_lock(file: File) -> Result<Self, String> {
        Ok(Self { _file: file })
    }

    pub(crate) fn detach_without_unlock(self) -> File {
        let file = unsafe { std::ptr::read(&self._file) };
        std::mem::forget(self);
        file
    }

    pub(crate) fn from_locked_file(file: File) -> Self {
        Self { _file: file }
    }
}

#[derive(Debug)]
pub(crate) struct DaemonLockFiles {
    _persistent: Option<DaemonLockFile>,
    _legacy: DaemonLockFile,
}

impl DaemonLockFiles {
    /// Detaches both lock files without invoking `LOCK_UN`, preserving held locks
    /// across handover ownership transfer.
    pub(crate) fn detach_without_unlock(self) -> (Option<File>, File) {
        let mut me = std::mem::ManuallyDrop::new(self);
        let persistent = me._persistent.take().map(|p| p.detach_without_unlock());
        let legacy = unsafe { std::ptr::read(&me._legacy) }.detach_without_unlock();
        (persistent, legacy)
    }

    /// Adopts pre-locked files received via handover descriptor transfer.
    pub(crate) fn from_locked_files(persistent: Option<File>, legacy: File) -> Self {
        Self {
            _persistent: persistent.map(DaemonLockFile::from_locked_file),
            _legacy: DaemonLockFile::from_locked_file(legacy),
        }
    }

    /// Raw descriptors of the held locks (persistent, legacy), for duplication into an
    /// authority transfer. The returned fds stay owned by self.
    #[cfg(unix)]
    pub(crate) fn raw_fds(&self) -> (Option<std::os::fd::RawFd>, std::os::fd::RawFd) {
        use std::os::fd::AsRawFd;
        (
            self._persistent.as_ref().map(|lock| lock._file.as_raw_fd()),
            self._legacy._file.as_raw_fd(),
        )
    }
}

fn try_lock_file(file: File) -> Result<DaemonLockFile, String> {
    DaemonLockFile::try_lock(file)
}

/// Takes one lock file, optionally outlasting a predecessor that is still shutting down. The
/// wait is only ever armed for a declared successor, so ordinary startup keeps failing fast.
/// The deadline is checked before every attempt, so a closed window can never be reopened by a
/// lock that happens to free up after it.
fn lock_file_with_optional_wait(
    path: &Path,
    deadline: Option<std::time::Instant>,
) -> Result<DaemonLockFile, String> {
    lock_file_with_optional_wait_internal(path, deadline, || {})
}

fn lock_file_with_optional_wait_internal<F: FnMut()>(
    path: &Path,
    deadline: Option<std::time::Instant>,
    mut on_pre_attempt: F,
) -> Result<DaemonLockFile, String> {
    let mut last_error: Option<String> = None;
    loop {
        let Some(deadline) = deadline else {
            let file = open_secure_lock_file(path)?;
            return try_lock_file(file);
        };
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(last_error.unwrap_or_else(|| {
                format!(
                    "Timed out waiting for the daemon instance lock at {}",
                    path.display()
                )
            }));
        }
        on_pre_attempt();
        let file = open_secure_lock_file(path)?;
        match try_lock_file(file) {
            Ok(locked) => {
                if std::time::Instant::now() >= deadline {
                    drop(locked);
                    return Err(format!(
                        "Acquired instance lock at {} after the wait window expired",
                        path.display()
                    ));
                }
                return Ok(locked);
            }
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(remaining.min(std::time::Duration::from_millis(200)));
            }
        }
    }
}

pub(crate) fn acquire_daemon_locks(
    persistent_path: Option<&Path>,
    legacy_path: &Path,
) -> Result<DaemonLockFiles, String> {
    let deadline = crate::daemon::handover::successor_wait_from_env()
        .map(|wait| std::time::Instant::now() + wait);
    let persistent = if let Some(path) = persistent_path {
        let parent = path
            .parent()
            .ok_or_else(|| format!("Persistent lock path has no parent: {}", path.display()))?;
        ensure_runtime_directory(parent)?;
        let locked = lock_file_with_optional_wait(path, deadline)?;
        Some(locked)
    } else {
        None
    };

    let locked = lock_file_with_optional_wait(legacy_path, deadline)?;
    Ok(DaemonLockFiles {
        _persistent: persistent,
        _legacy: locked,
    })
}

fn remove_stale_socket_after_lock(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            validate_safe_ownership_and_type(path, RuntimeNodeKind::Socket)?;
            fs::remove_file(path).map_err(|error| {
                format!("Failed to remove stale socket {}: {error}", path.display())
            })
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to inspect daemon socket: {error}")),
    }
}

#[cfg(test)]
mod endpoint_artifact_tests {
    use super::*;

    fn publish(path: &Path, contents: &str) {
        #[cfg(unix)]
        {
            let _ = contents;
            // A replaced socket is renamed aside, not unlinked, so its inode stays allocated and
            // the new bind cannot reuse it: the replacement has a different identity for sure.
            if path.exists() {
                fs::rename(path, path.with_extension("replaced")).unwrap();
            }
            // A bound socket, then its listener closed: the path stays, as after a retire.
            drop(std::os::unix::net::UnixListener::bind(path).unwrap());
        }
        #[cfg(not(unix))]
        fs::write(path, contents).unwrap();
    }

    /// A clean exit removes the endpoint file this owner published (run-r6d: a daemon.port
    /// left behind kept every later GUI from starting an owner) and never one a later owner
    /// replaced at the same path.
    #[test]
    fn only_endpoint_files_that_still_name_this_owner_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("daemon.endpoint");
        let replaced = dir.path().join("control.endpoint");
        publish(&own, "51970");
        publish(&replaced, "51971");
        let recorded: Vec<EndpointArtifact> = [&own, &replaced]
            .iter()
            .map(|path| EndpointArtifact { path: path.to_path_buf(), identity: endpoint_artifact_identity(path).unwrap() })
            .collect();
        // A later owner publishes its own endpoint at the second path.
        publish(&replaced, "53058");
        assert_eq!(remove_owned_endpoint_artifacts(&recorded), vec![own.clone()]);
        assert!(!own.exists(), "this owner's endpoint must be gone");
        assert!(replaced.exists(), "a later owner's endpoint must survive");
    }
}

/// What makes a public endpoint file this owner's: on Unix the socket inode it bound (a later
/// owner binds a new one), elsewhere the port or token the file carries (a later owner writes
/// its own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EndpointIdentity {
    #[cfg(unix)]
    Inode { dev: u64, ino: u64 },
    #[cfg(not(unix))]
    Contents(String),
}

#[derive(Debug, Clone)]
pub(crate) struct EndpointArtifact {
    pub path: PathBuf,
    pub identity: EndpointIdentity,
}

pub(crate) fn endpoint_artifact_identity(path: &Path) -> Option<EndpointIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::symlink_metadata(path).ok()?;
        Some(EndpointIdentity::Inode { dev: meta.dev(), ino: meta.ino() })
    }
    #[cfg(not(unix))]
    {
        fs::read_to_string(path).ok().map(EndpointIdentity::Contents)
    }
}

/// Removes each recorded endpoint file that still names this daemon; a file a later owner
/// replaced is left alone. Returns the removed paths. Blocking.
pub(crate) fn remove_owned_endpoint_artifacts(artifacts: &[EndpointArtifact]) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for artifact in artifacts {
        if endpoint_artifact_identity(&artifact.path).as_ref() != Some(&artifact.identity) {
            continue;
        }
        match fs::remove_file(&artifact.path) {
            Ok(()) => removed.push(artifact.path.clone()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, path = %artifact.path.display(), "Failed to remove endpoint artifact"),
        }
    }
    removed
}

/// Runtime file carrying this boot's daemon transport token, beside the `daemon.port` file the
/// loopback listener publishes. A loopback TCP port has no filesystem ownership check, so the
/// published port alone must not be enough to drive the daemon.
#[cfg(not(unix))]
pub fn get_transport_token_path() -> PathBuf {
    get_runtime_dir().join("daemon.token")
}

/// Length of every generated transport token.
#[cfg(not(unix))]
const TRANSPORT_TOKEN_LEN: usize = 48;

/// One per-boot bearer token, drawn from the crate's CSPRNG.
#[cfg(not(unix))]
fn generate_transport_token() -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;

    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TRANSPORT_TOKEN_LEN)
        .map(char::from)
        .collect()
}

/// Publishes this boot's loopback rendezvous pair in the order a reader depends on: the previous
/// boot's token is dropped first, this boot's token is written second, and the port is published
/// last. The ordering guarantees that this boot's port is never on disk before this boot's token.
/// It does not extend to a port a predecessor left behind: while this boot is republishing, a
/// reader can still find that port with no token beside it, an interval that is harmless because
/// the daemon rejects a credential it did not mint and `DaemonClient` re-reads both halves once.
///
/// The stale port is dropped by the caller, not here: `remove_stale_socket_after_lock` removes it
/// under the instance lock before the listener binds. The ordering does not make the pair atomic,
/// so a reader can still assemble this boot's token beside a predecessor's port; that pair is made
/// harmless by the daemon rejecting a credential it did not mint and by `DaemonClient` re-reading
/// both halves once.
///
/// `on_token_published` observes the instant between the two writes, which is the only instant
/// that decides the contract; the ordering test in `agent_state_transport_tests` pins it without
/// a daemon by reading both files there.
#[cfg(any(not(unix), test))]
fn publish_transport_rendezvous_internal<F: FnMut()>(
    port_path: &Path,
    token_path: &Path,
    port: u16,
    token: &str,
    mut on_token_published: F,
) -> std::io::Result<()> {
    // The previous boot's token goes first: while it is gone and the port has not been rewritten,
    // a reader's pair is absent rather than mismatched, so it re-reads both halves instead of
    // presenting a credential that cannot belong to the port it read.
    match fs::remove_file(token_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, "Failed to remove stale daemon transport token"),
    }
    fs::write(token_path, token)?;
    on_token_published();
    fs::write(port_path, port.to_string())
}

/// Publishes the pair with no observer attached.
#[cfg(not(unix))]
fn publish_transport_rendezvous(
    port_path: &Path,
    token_path: &Path,
    port: u16,
    token: &str,
) -> std::io::Result<()> {
    publish_transport_rendezvous_internal(port_path, token_path, port, token, || {})
}

/// Constant-time comparison of a presented token against the expected one. An absent or empty
/// token never matches, so a client that omits the credential is rejected exactly like one that
/// guesses wrong. Compiled for the loopback transport and for the tests that pin it.
#[cfg(any(not(unix), test))]
fn transport_token_matches(expected: &str, presented: Option<&str>) -> bool {
    let Some(presented) = presented.filter(|token| !token.is_empty()) else {
        return false;
    };
    if presented.len() != expected.len() {
        return false;
    }
    presented
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (presented, expected)| {
            acc | (presented ^ expected)
        })
        == 0
}

/// Reads the bearer token out of one raw protocol line. The token travels beside the payload as
/// `token`, the shape the agent extension's TCP mode already writes for remote sessions, so no
/// wire struct has to carry it.
#[cfg(any(not(unix), test))]
fn transport_token_from_line(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    value.get("token")?.as_str().map(str::to_owned)
}

pub fn get_agent_state_socket_path() -> PathBuf {
    get_runtime_dir().join("agent-state.sock")
}

pub fn agent_state_socket_path() -> String {
    get_agent_state_socket_path().to_string_lossy().into_owned()
}

/// One parsed agent-state ingress report: session id, state, agent, provider session, detail.
type AgentStateIngressReport = (
    String,
    String,
    Option<String>,
    Option<AgentProviderSession>,
    Option<String>,
);

/// Agents whose built-in herdr reporter Ferryx accepts on the agent-state socket. Every pane gets
/// `HERDR_ENV`, so any herdr-aware agent may report there; only an agent named here, and measured
/// to send the `pane.report_agent` envelope, is turned into state. crush: `internal/herdr/client.go`.
#[cfg(any(unix, test))]
const HERDR_REPORTING_AGENTS: &[&str] = &["crush"];

/// Translates one herdr `pane.report_agent` request into Ferryx's agent-state report.
///
/// The pane id is the Ferryx session id (`HERDR_PANE_ID` is set to `FERRYX_SESSION_ID`), and a
/// non-empty `agent_session_id` is the provider session the agent resumes with. `pane.release_agent`
/// carries no state and is not translated: publishing `idle` on release would replace the retained
/// provider session, and the foreground watcher already releases a pane whose agent exited.
#[cfg(any(unix, test))]
fn translate_herdr_report(line: &str) -> Option<AgentStateReport> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("method")?.as_str()? != "pane.report_agent" {
        return None;
    }
    let params = value.get("params")?;
    let agent = params.get("agent")?.as_str()?.trim().to_ascii_lowercase();
    if !HERDR_REPORTING_AGENTS.contains(&agent.as_str()) {
        return None;
    }
    let session_id = params.get("pane_id")?.as_str()?.trim();
    if session_id.is_empty() {
        return None;
    }
    let provider_session = params
        .get("agent_session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| AgentProviderSession {
            key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
            id: id.to_string(),
            transcript_path: None,
        });
    Some(AgentStateReport {
        session_id: session_id.to_string(),
        state: params.get("state")?.as_str()?.to_string(),
        agent: Some(agent),
        provider_session,
        detail: None,
    })
}

/// The reply line a herdr client reads, and discards, after each request it sends. Any `pane.*`
/// request is answered, including ones Ferryx does not translate, so the client never waits out
/// its timeout. Lines without a `method` are Ferryx's own reports, which expect no reply.
#[cfg(any(unix, test))]
fn herdr_response_line(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if !value.get("method")?.as_str()?.starts_with("pane.") {
        return None;
    }
    let id = value.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let mut response = serde_json::json!({ "id": id, "result": {} }).to_string();
    response.push('\n');
    Some(response)
}

/// How the ingress must present one report's provider identity to admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IngressAdmissionIdentity {
    /// Admit the claim exactly as the report carried it.
    AsReported,
    /// Admit the claim without its pinned path: this family resolves its store from the id alone.
    WithoutPath,
    /// Admit no identity for this report.
    Refused,
}

/// What the ingress did with one report, which decides what the hub may publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IngressOutcome {
    /// The session is not machine-owned, so the local consumer owns its metadata.
    Local,
    /// Admission accepted the identity.
    Admitted,
    /// Admission refused it.
    Refused,
}

/// Agents whose CLI leaves no process-bound record of a fresh session. Measured on claude
/// 2.1.308 (re-explore-5): the transcript is never held open (0/40 lsof samples while working, none
/// at idle) and a fresh launch carries no id in argv. Only a resumed claude names its session
/// (`--resume <uuid>`, read by `ipc::agents::resume_id_from_args`). For these agents the agent's
/// own store file plus the store-recorded cwd stand in for the missing process proof.
/// omp, measured on omarchy 2026-10-23 (`.omo-parity/facts/omp-session-fd-2026-10-23.md`,
/// re-explore-6 §2): it keeps one persistent append handle on its session jsonl only after its first
/// write in that process. A fresh pane before its first turn, or a resumed/reconnected pane while
/// idle, holds no session file open (0/35 /proc fd samples), and `/new` switches the session
/// under an unchanged argv.
/// cline 3.0.73, measured on this Mac (re-explore-7 G1): a fresh pane holds no session file open
/// (0/45 lsof samples on the TUI and hub pids) and carries no `--id` in argv.
/// crush (re-explore-7 G2): every session lives in one shared `.crush/crush.db`, so no open file
/// names one session, and a fresh launch has no `--session` in argv.
const AGENTS_WITHOUT_PROCESS_BOUND_SESSION: [&str; 4] = ["claude", "omp", "cline", "crush"];

pub(crate) fn agent_has_no_process_bound_session(agent: &str) -> bool {
    AGENTS_WITHOUT_PROCESS_BOUND_SESSION.contains(&agent)
}

/// The discovered id that may bind (and veto) a report. For an agent with no process-bound
/// session, an id read only from the launch argv is dropped: after `/clear`, `/new` or an in-TUI
/// `/resume` the agent writes a new session while its argv still names the old one (measured on
/// claude 2.1.309, re-explore-6 §4). The report is then proven by its own store file instead.
pub(crate) fn binding_discovery(
    agent: &str,
    found: String,
    source: crate::ipc::agents::DiscoverySource,
) -> Option<String> {
    let launch_only = source == crate::ipc::agents::DiscoverySource::LaunchArgv;
    (!(launch_only && agent_has_no_process_bound_session(agent))).then_some(found)
}

/// The admission decision for one report's provider claim.
///
/// A claim from a family the machine publishes (`resume_cwd::MACHINE_RESUME_AGENTS`) can lose its
/// pinned path, and only when the id is the one discovery read from the pane's own process AND the
/// claimed path is that agent's own store file for that id on this host. Every family in that list
/// resumes from the id alone (`terminal/shell.rs` `resolve_agent_resume_plan`; omp falls back to the
/// id when no path is given), so a verified path carries nothing the machine needs. A family outside
/// that list keeps its claim exactly as it arrived, so the existing pinned-path rule still decides it.
fn ingress_admission_identity(
    agent: &str,
    id: &str,
    has_path: bool,
    discovered: Option<&str>,
    path_is_own_store: bool,
) -> IngressAdmissionIdentity {
    if !has_path || !crate::terminal::resume_cwd::machine_admits_agent(agent) {
        return IngressAdmissionIdentity::AsReported;
    }
    // An agent whose CLI keeps no process-bound record of its session (no open store file, no
    // argv on a fresh launch) can never be discovered; its own store file for the reported id is
    // then the proof, and admit_agent_report still requires that store's cwd to be this pane's.
    // A discovered id that differs is still refused.
    let discovery_unavailable = discovered.is_none() && agent_has_no_process_bound_session(agent);
    if (discovered.is_some_and(|found| found == id.trim()) || discovery_unavailable)
        && path_is_own_store
    {
        return IngressAdmissionIdentity::WithoutPath;
    }
    // The claim names a path this session cannot prove is its own store: admit no identity rather
    // than a path nothing verified.
    IngressAdmissionIdentity::Refused
}

/// Is `claimed` the store file `agent` itself keeps for session `id`?
///
/// The claim must be a regular file inside that agent's own history root (`roots`, from
/// `ipc::agent_history::provider_history_roots`), compared after canonicalization so a symlink
/// cannot pass by spelling, and the agent's own path reader (the one discovery uses) must read
/// exactly `id` from it. crush keeps every session in one shared database, so no file proves one
/// session and its claim is never accepted here; crush reports carry no path anyway.
fn ingress_path_is_agent_store(
    agent: &str,
    id: &str,
    claimed: &Path,
    roots: &[(crate::scoped_contracts::CanonicalProvider, PathBuf)],
) -> bool {
    let wire = if agent == "cursor-agent" { "cursor" } else { agent };
    let Some(provider) = crate::agent_history::history::provider_from_wire(wire) else {
        return false;
    };
    let Ok(claimed) = std::fs::canonicalize(claimed) else {
        return false;
    };
    if !claimed.is_file() {
        return false;
    }
    let in_own_root = roots
        .iter()
        .filter(|(candidate, _)| *candidate == provider)
        .filter_map(|(_, root)| std::fs::canonicalize(root).ok())
        .any(|root| claimed.starts_with(&root));
    in_own_root
        && crate::ipc::agents::extract_session_id_from_path(wire, &claimed.to_string_lossy())
            .is_some_and(|found| found == id.trim())
}

/// The identity the hub may publish for one report.
fn ingress_publish_identity(
    outcome: IngressOutcome,
    reported: Option<AgentProviderSession>,
    admitted: Option<AgentProviderSession>,
) -> Option<AgentProviderSession> {
    match outcome {
        // A local pane keeps the report as reported: its consumer resolves the transcript from it.
        IngressOutcome::Local => reported,
        IngressOutcome::Admitted => admitted,
        // A refused identity is never published, so a stale or foreign claim cannot linger.
        IngressOutcome::Refused => None,
    }
}

/// First wait between process-discovery probes of one machine-pane agent (re-explore-4 G2). It
/// matches the UI's `PROVIDER_SESSION_PROBE_INTERVAL_MS` floor for local panes.
const MACHINE_IDENTITY_PROBE_FLOOR: Duration = Duration::from_secs(2);
/// Longest wait between probes. kimi and gjc create their session file only after the first
/// persisted message, so a pane can sit at a fresh prompt for a while before discovery can work.
const MACHINE_IDENTITY_PROBE_CEILING: Duration = Duration::from_secs(60);
/// Probes per agent process before the observer gives up on it; a new agent process re-arms it.
const MACHINE_IDENTITY_PROBE_ATTEMPTS: u32 = 40;

/// Should the foreground observer try process discovery for this pane's agent? Only a family the
/// machine publishes (`MACHINE_RESUME_AGENTS`), only on a machine-owned pane (a local pane's UI
/// probes for itself), and never while the pane already holds an identity discovery must not
/// replace (`agent_state::holds_live_provider`).
fn machine_identity_probe_wanted(agent: &str, machine_only: bool, holds_provider: bool) -> bool {
    crate::terminal::resume_cwd::machine_admits_agent(agent) && machine_only && !holds_provider
}

/// Wait before probe number `attempts` (0-based), or `None` once the budget is spent. Doubles
/// from the floor up to the ceiling.
fn machine_identity_probe_delay(attempts: u32) -> Option<Duration> {
    if attempts >= MACHINE_IDENTITY_PROBE_ATTEMPTS {
        return None;
    }
    let doubled = MACHINE_IDENTITY_PROBE_FLOOR.saturating_mul(1u32 << attempts.min(16));
    Some(doubled.min(MACHINE_IDENTITY_PROBE_CEILING))
}

/// The report the observer feeds through admission for a discovered provider session. The state
/// word is the retained one, since discovery observes no activity (`idle` when nothing is
/// retained), and the claim carries no path: every `MACHINE_RESUME_AGENTS` member resumes from
/// the id alone, and `admit_agent_report` refuses a pinned path.
fn discovered_machine_report(
    session_id: &str,
    agent: &str,
    provider_id: &str,
    retained: Option<&AgentState>,
) -> AgentStateIngressReport {
    (
        session_id.to_string(),
        retained.map_or_else(|| "idle".to_string(), |state| state.state.clone()),
        Some(agent.to_string()),
        Some(AgentProviderSession {
            key: AgentProviderSessionKey::SessionId,
            id: provider_id.to_string(),
            transcript_path: None,
        }),
        retained.and_then(|state| state.detail.clone()),
    )
}

/// The observer's probe bookkeeping for one pane's current agent process.
struct MachineIdentityProbe {
    agent_pid: u32,
    agent: &'static str,
    /// Provider session retained when this agent process was first seen.
    baseline: Option<AgentProviderSession>,
    attempts: u32,
    next_at: tokio::time::Instant,
    in_flight: bool,
    finished: bool,
}

/// Name of the rendezvous record the non-unix ingress publishes beside its socket: the loopback
/// port on the first line, the bearer token a pane must present on the second.
const AGENT_STATE_RENDEZVOUS_FILE: &str = "agent-state.rendezvous";

/// Runtime record carrying the non-unix ingress endpoint. `apply_agent_state_tcp_env` reads
/// exactly this path as a pane spawns.
pub fn get_agent_state_rendezvous_path() -> PathBuf {
    get_runtime_dir().join(AGENT_STATE_RENDEZVOUS_FILE)
}

/// Publishes the ingress endpoint as one record renamed into place: a reader can never observe
/// this boot's port beside a previous boot's token, or the reverse.
///
/// `fs::rename` replaces an existing destination on every target: Unix `rename(2)`, and Windows
/// `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` (see `library/std/src/sys/fs/windows.rs`),
/// matching the `std::fs::rename` documentation's "replacing the original file if `to` already
/// exists". A Windows reader does not block the replace: std opens files with a default share
/// mode of `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.
#[cfg(any(not(unix), test))]
fn publish_agent_state_rendezvous(
    runtime_dir: &Path,
    port: u16,
    token: &str,
) -> std::io::Result<()> {
    let staged = runtime_dir.join(format!("{AGENT_STATE_RENDEZVOUS_FILE}.tmp"));
    fs::write(&staged, format!("{port}\n{token}\n"))?;
    fs::rename(&staged, runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE))
}

/// Publishes the rendezvous record and only then records the endpoint the accessor reports, so an
/// endpoint is observable exactly when a pane can discover the pair from disk. A publish that
/// fails leaves the endpoint untouched rather than reporting a pair that was never made
/// discoverable.
#[cfg(any(not(unix), test))]
fn publish_agent_state_endpoint(
    endpoint: &Mutex<Option<(u16, String)>>,
    runtime_dir: &Path,
    port: u16,
    token: &str,
) -> std::io::Result<()> {
    publish_agent_state_rendezvous(runtime_dir, port, token)?;
    *endpoint.lock() = Some((port, token.to_owned()));
    Ok(())
}

/// Drops a previous boot's rendezvous record, including the two-file pair older builds published.
/// A listener that fails to bind or publish must not leave a stale pair looking valid.
#[cfg(any(not(unix), test))]
fn clear_stale_agent_state_rendezvous(runtime_dir: &Path) {
    let staged = format!("{AGENT_STATE_RENDEZVOUS_FILE}.tmp");
    for name in [
        AGENT_STATE_RENDEZVOUS_FILE,
        staged.as_str(),
        "agent-state.port",
        "agent-state.token",
    ] {
        match fs::remove_file(runtime_dir.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                %error,
                file = name,
                "Failed to remove stale agent state rendezvous"
            ),
        }
    }
}

pub(crate) use super::session_service::normalize_process_cwd;

fn daemon_ssh_store_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR") {
        return PathBuf::from(dir).join("ssh_hosts.json");
    }
    let base = dirs_next()
        .unwrap_or_else(get_runtime_dir)
        .join("com.ferryx.app");
    if is_dev_runtime() {
        base.join("dev/ssh_hosts.json")
    } else {
        base.join("ssh_hosts.json")
    }
}

pub struct DaemonServer {
    paired_hosts: crate::paired_host::service::PairedHostService,
    pub session_router: Arc<crate::daemon::proxy::SessionRouter>,
    pub handover_manager: Arc<crate::daemon::handover::HandoverManager>,
    resource_sampler: Arc<crate::daemon::resource_usage::ResourceSampler>,
    terminal_service: Arc<TerminalService>,
    workspace_registry: WorkspaceRegistry,
    remote_state: Arc<RemoteGatewayState>,
    remote_server_handle: Arc<Mutex<Option<RemoteServerHandle>>>,
    remote_configure_lock: tokio::sync::Mutex<()>,
    epoch: u64,
    binary_path: Option<String>,
    binary_mtime_ms: Option<u64>,
    pub(crate) session_service: Arc<DaemonSessionService>,
    /// Loopback TCP endpoint (`port`, bearer `token`) of the agent-state ingress, recorded once
    /// that ingress published its rendezvous record. `None` on unix, whose ingress is the socket
    /// path [`agent_state_socket_path`] returns, and `None` on every platform until then: every
    /// failure to bind or publish leaves it `None`, so it is `Some` only while a pane can discover
    /// the pair from disk.
    agent_state_endpoint: Mutex<Option<(u16, String)>>,
    /// Set once this boot's agent-state ingress was observed to fail. It is the only thing that
    /// separates "not published because binding or publishing FAILED" from "not published yet":
    /// the endpoint above is `None` in both cases, so a caller reporting an absent endpoint needs
    /// this flag to know whether it is looking at a degradation.
    agent_state_ingress_unavailable: std::sync::atomic::AtomicBool,
    /// Whether panes get the agent-state endpoint and boot installs the extension. Shared with
    /// the PTY manager, so a change applies to the next pane without a restart.
    agent_integration: Arc<crate::daemon::agent_extension::AgentIntegration>,
    /// Bearer token every connection must present before a request is dispatched. Windows has no
    /// filesystem ownership boundary on the transport, so the published port alone cannot
    /// authenticate a client.
    #[cfg(not(unix))]
    transport_token: String,
    /// Public endpoint files this daemon published or serves, removed on a clean exit while it
    /// is still the owner (shutdown_gracefully).
    published_endpoints: Mutex<Vec<EndpointArtifact>>,
    #[cfg(test)]
    helper_home: Option<String>,
    remote_event_tx: broadcast::Sender<DaemonRemoteEvent>,
    #[cfg(test)]
    _catalog_fixture: Option<tempfile::TempDir>,
    /// Test-only park point for a Close handler: after its response is flushed and before its
    /// RetirementGuard drops, the handler notifies .0 ("parked") and waits on .1 ("release").
    #[cfg(test)]
    close_park: std::sync::OnceLock<Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
}

#[cfg(test)]
mod agent_state_transport_tests {
    use super::*;

    #[test]
    fn transport_token_matches_only_the_exact_credential() {
        let token = "9Zq3r8Tf2kLp";
        assert!(transport_token_matches(token, Some(token)));
        assert!(!transport_token_matches(token, None));
        assert!(!transport_token_matches(token, Some("")));
        assert!(!transport_token_matches(token, Some("9Zq3r8Tf2kL")));
        assert!(!transport_token_matches(token, Some("9Zq3r8Tf2kLq")));
        assert!(!transport_token_matches(token, Some("9Zq3r8Tf2kLx")));
    }

    #[test]
    fn transport_token_is_read_from_the_request_line() {
        assert_eq!(
            transport_token_from_line(r#"{"type":"handshake","version":5,"token":"abc"}"#)
                .as_deref(),
            Some("abc")
        );
        assert_eq!(
            transport_token_from_line(
                r#"{"type":"agentState","sessionId":"s","state":"idle","token":"abc"}"#
            )
            .as_deref(),
            Some("abc")
        );
        assert_eq!(
            transport_token_from_line(r#"{"type":"handshake","version":5}"#),
            None
        );
        assert_eq!(
            transport_token_from_line(r#"{"type":"handshake","token":42}"#),
            None
        );
        assert_eq!(transport_token_from_line("not json"), None);
    }

    /// The envelope crush's `internal/herdr` client sends, as measured on omarchy.
    fn crush_report(state: &str, agent_session_id: &str) -> String {
        format!(
            r#"{{"id":"crush:report:1761000000000000000","method":"pane.report_agent","params":{{"pane_id":"pane-7","source":"crush","agent":"crush","state":"{state}","seq":3,"agent_session_id":"{agent_session_id}"}}}}"#
        )
    }

    #[test]
    fn crush_herdr_reports_translate_every_measured_state() {
        let session = "3df112d9-e56f-4f69-b8bb-c58b5d580e89";
        for state in ["idle", "working", "blocked"] {
            let (session_id, parsed_state, agent, provider, detail) =
                DaemonServer::parse_herdr_report(&crush_report(state, session))
                    .unwrap_or_else(|| panic!("crush {state} report must be accepted"));
            assert_eq!(session_id, "pane-7", "pane_id is the Ferryx session id");
            assert_eq!(parsed_state, state);
            assert_eq!(agent.as_deref(), Some("crush"));
            let provider = provider.expect("agent_session_id becomes the provider session");
            assert_eq!(provider.key, AgentProviderSessionKey::SessionId);
            assert_eq!(provider.id, session);
            assert_eq!(provider.transcript_path, None);
            assert_eq!(detail, None);
        }
        // The first report crush sends, before any prompt, has an empty session id.
        let (_, state, _, provider, _) =
            DaemonServer::parse_herdr_report(&crush_report("idle", "")).expect("init report");
        assert_eq!(state, "idle");
        assert_eq!(provider, None, "an empty agent_session_id is not a provider session");
    }

    #[test]
    fn herdr_lines_outside_the_crush_report_are_not_translated() {
        let release = r#"{"id":"crush:release:1","method":"pane.release_agent","params":{"pane_id":"pane-7","source":"crush","agent":"crush"}}"#;
        assert_eq!(DaemonServer::parse_herdr_report(release), None);
        assert!(
            herdr_response_line(release).is_some(),
            "release is still answered so crush does not wait out its timeout"
        );
        let unknown_agent = crush_report("working", "s").replace(r#""agent":"crush""#, r#""agent":"other""#);
        assert_eq!(DaemonServer::parse_herdr_report(&unknown_agent), None);
        assert_eq!(DaemonServer::parse_herdr_report(&crush_report("bogus", "s")), None);
        assert_eq!(
            DaemonServer::parse_herdr_report(&crush_report("idle", "s").replace("pane-7", "")),
            None
        );
        // Ferryx's own report is neither a herdr request nor answered.
        let own = r#"{"type":"agentState","sessionId":"s1","state":"idle"}"#;
        assert_eq!(DaemonServer::parse_herdr_report(own), None);
        assert_eq!(herdr_response_line(own), None);
    }

    #[test]
    fn a_herdr_request_is_answered_with_one_json_line_echoing_its_id() {
        let response = herdr_response_line(&crush_report("working", "s")).expect("reply");
        assert!(response.ends_with('\n') && response.matches('\n').count() == 1);
        let value: serde_json::Value = serde_json::from_str(response.trim()).expect("json");
        assert_eq!(value["id"], "crush:report:1761000000000000000");
    }

    #[test]
    fn the_port_is_published_only_after_the_token_that_authenticates_it() {
        let dir = tempfile::tempdir().unwrap();
        let port_path = dir.path().join("daemon.port");
        let token_path = dir.path().join("daemon.token");
        // The pair a previous boot left on disk, as a reader finds it mid-restart.
        std::fs::write(&port_path, "41111").unwrap();
        std::fs::write(&token_path, "previous-boot-token").unwrap();

        // Observed between the two writes: a new port visible here would mean a reader can pair a
        // new port with the token the previous boot published beside the old one.
        let observed = std::cell::RefCell::new(None);
        publish_transport_rendezvous_internal(
            &port_path,
            &token_path,
            52222,
            "this-boot-token",
            || {
                *observed.borrow_mut() = Some((
                    std::fs::read_to_string(&port_path).unwrap(),
                    std::fs::read_to_string(&token_path).unwrap(),
                ));
            },
        )
        .unwrap();

        let (port_between_writes, token_between_writes) = observed.into_inner().unwrap();
        assert_eq!(
            port_between_writes, "41111",
            "the port must still be the predecessor's when this boot's token becomes readable, \
             otherwise a reader finds a port whose credential is not yet on disk"
        );
        assert_eq!(token_between_writes, "this-boot-token");
        assert_eq!(
            std::fs::read_to_string(&token_path).unwrap(),
            "this-boot-token"
        );
        assert_eq!(std::fs::read_to_string(&port_path).unwrap(), "52222");
    }

    #[test]
    fn agent_state_endpoint_is_absent_until_the_ingress_binds() {
        let server = DaemonServer::new_with_paths(None, None);
        assert_eq!(server.agent_state_endpoint(), None);
    }

    #[test]
    fn a_failed_rendezvous_publish_leaves_no_endpoint_to_report() {
        let runtime_dir = std::env::temp_dir().join(format!(
            "ferryx-endpoint-unpublished-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&runtime_dir).ok();

        // An absent runtime directory is the publish failure the ingress reports as unavailable.
        // The endpoint must stay absent, or a caller observes an unavailable ingress beside an
        // endpoint that no pane can discover.
        let endpoint = Mutex::new(None);
        assert!(
            publish_agent_state_endpoint(&endpoint, &runtime_dir, 41_234, "token-abc").is_err(),
            "publishing into an absent runtime directory must fail"
        );
        assert_eq!(
            *endpoint.lock(),
            None,
            "an unpublished rendezvous must never be reported as an endpoint"
        );
        assert!(!runtime_dir.exists());

        // The endpoint becomes observable only with the record it names, and it names the pair
        // that record carries.
        fs::create_dir_all(&runtime_dir).expect("create runtime dir");
        publish_agent_state_endpoint(&endpoint, &runtime_dir, 41_234, "token-abc")
            .expect("publish endpoint");
        assert_eq!(*endpoint.lock(), Some((41_234, "token-abc".to_string())));
        assert_eq!(
            fs::read_to_string(runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE))
                .expect("read published record"),
            "41234\ntoken-abc\n"
        );

        fs::remove_dir_all(&runtime_dir).ok();
    }

    #[test]
    fn readiness_is_released_only_after_the_ingress_failure_is_recorded() {
        let server = DaemonServer::new_with_paths(None, None);
        let (ready_tx, mut ready_rx) = tokio::sync::oneshot::channel();
        // Read from inside the readiness signal itself, so the assertion is on the ordering and
        // not on the final state: a boot that signals first and records afterwards observes
        // `false` here and fails.
        let recorded_before_signal = std::cell::Cell::new(None);
        let probe = &server;
        probe.settle_agent_state_ingress(None, || {
            recorded_before_signal.set(Some(probe.agent_state_ingress_unavailable()));
            let _ = ready_tx.send(());
        });
        assert_eq!(
            recorded_before_signal.get(),
            Some(true),
            "the failed ingress must be recorded before readiness is released"
        );
        assert!(
            server.agent_state_ingress_unavailable(),
            "a boot without a usable ingress must record it rather than leave callers to infer it"
        );
        assert!(
            ready_rx.try_recv().is_ok(),
            "readiness must still be signalled: terminals keep working without the ingress"
        );
    }

    #[tokio::test]
    async fn a_bound_ingress_is_not_recorded_as_unavailable() {
        let server = DaemonServer::new_with_paths(None, None);
        let (ready_tx, mut ready_rx) = tokio::sync::oneshot::channel();
        // A live handle stands in for an ingress that bound and published its endpoint.
        let ingress = Some(tokio::spawn(async {}));
        let probe = &server;
        probe.settle_agent_state_ingress(ingress, || {
            let _ = ready_tx.send(());
        });
        assert!(
            !server.agent_state_ingress_unavailable(),
            "an ingress that bound must never be reported as a failure"
        );
        assert!(
            ready_rx.try_recv().is_ok(),
            "readiness must still be signalled when the ingress bound"
        );
    }

    #[test]
    fn agent_state_rendezvous_record_sits_beside_the_socket() {
        // The non-unix ingress publishes its loopback port and bearer token to this one record,
        // and `apply_agent_state_tcp_env` reads exactly it before a pane spawns.
        let socket = get_agent_state_socket_path();
        let runtime_dir = socket.parent().expect("agent state socket has a parent");
        assert_eq!(
            get_agent_state_rendezvous_path(),
            runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE)
        );
    }

    #[test]
    fn rendezvous_publish_writes_the_port_and_the_token_as_one_file() {
        let runtime_dir =
            std::env::temp_dir().join(format!("ferryx-rendezvous-{}", std::process::id()));
        fs::create_dir_all(&runtime_dir).expect("create rendezvous dir");
        clear_stale_agent_state_rendezvous(&runtime_dir);

        publish_agent_state_rendezvous(&runtime_dir, 41_234, "token-abc")
            .expect("publish rendezvous");
        let record = runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE);
        assert_eq!(
            fs::read_to_string(&record).expect("read published rendezvous"),
            "41234\ntoken-abc\n",
            "one record must carry both the port and the token"
        );
        // The pair exists under a single name, and the staged file the rename consumed is gone.
        assert!(!runtime_dir.join("agent-state.port").exists());
        assert!(!runtime_dir.join("agent-state.token").exists());
        assert!(!runtime_dir.join(format!("{AGENT_STATE_RENDEZVOUS_FILE}.tmp")).exists());

        // A later boot replaces the record in place rather than leaving two halves behind.
        publish_agent_state_rendezvous(&runtime_dir, 41_235, "token-def")
            .expect("republish rendezvous");
        assert_eq!(
            fs::read_to_string(&record).expect("read republished rendezvous"),
            "41235\ntoken-def\n"
        );

        // Clearing drops the record and the two-file pair older builds published.
        fs::write(runtime_dir.join("agent-state.port"), "41234").expect("stage legacy port");
        fs::write(runtime_dir.join("agent-state.token"), "token-abc")
            .expect("stage legacy token");
        clear_stale_agent_state_rendezvous(&runtime_dir);
        assert!(!record.exists());
        assert!(!runtime_dir.join("agent-state.port").exists());
        assert!(!runtime_dir.join("agent-state.token").exists());

        fs::remove_dir_all(&runtime_dir).ok();
    }
}

#[cfg(all(test, unix))]
mod agent_integration_tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
    use tokio::net::UnixStream;

    async fn exchange(
        write_half: &mut OwnedWriteHalf,
        reader: &mut BufReader<OwnedReadHalf>,
        request: DaemonRequest,
    ) -> DaemonResponse {
        let mut json = serde_json::to_string(&request).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    /// Runs \`env\` in a new pane of \`server\` and returns everything it printed. The variable is
    /// seeded as an inherited value so its absence proves Ferryx withheld it.
    async fn pane_env(server: &DaemonServer) -> String {
        let mut cmd = portable_pty::CommandBuilder::new("/usr/bin/env");
        cmd.env("FERRYX_AGENT_STATE_SOCKET", "/inherited/should-not-survive.sock");
        let (_id, mut rx) = server
            .terminal_service
            .pty_manager()
            .spawn(cmd, 200, 50)
            .expect("spawn env pane");
        let mut out = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(chunk) = rx.recv().await {
                out.extend_from_slice(&chunk);
            }
        })
        .await
        .expect("env pane output ends when env exits");
        String::from_utf8_lossy(&out).into_owned()
    }

    fn server_at(dir: &Path) -> DaemonServer {
        DaemonServer::new_with_paths(
            Some(dir.join("remote-config.json")),
            Some(dir.join("remote-auth.json")),
        )
    }

    /// Runs `env` in a pane with no inherited herdr, returning what it printed.
    async fn pane_env_without_user_herdr(server: &DaemonServer) -> String {
        let mut cmd = portable_pty::CommandBuilder::new("/usr/bin/env");
        cmd.env_remove("HERDR_ENV");
        let (_id, mut rx) = server
            .terminal_service
            .pty_manager()
            .spawn(cmd, 200, 50)
            .expect("spawn env pane");
        let mut out = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(chunk) = rx.recv().await {
                out.extend_from_slice(&chunk);
            }
        })
        .await
        .expect("env pane output ends when env exits");
        String::from_utf8_lossy(&out).into_owned()
    }

    #[tokio::test]
    async fn panes_get_herdr_env_exactly_when_they_get_the_agent_state_socket() {
        let data = tempfile::tempdir().unwrap();
        let server = server_at(data.path());
        let env = pane_env_without_user_herdr(&server).await;
        assert!(env.contains("HERDR_ENV=1"), "{env}");
        assert!(
            env.contains(&format!("HERDR_SOCKET_PATH={}", agent_state_socket_path())),
            "{env}"
        );
        let session = env
            .lines()
            .find_map(|line| line.trim().strip_prefix("FERRYX_SESSION_ID="))
            .expect("pane has a session id")
            .to_owned();
        assert!(env.contains(&format!("HERDR_PANE_ID={session}")), "{env}");

        server.agent_integration.set(false).expect("disable integration");
        let env = pane_env_without_user_herdr(&server).await;
        assert!(env.contains("FERRYX_SESSION_ID="), "the pane ran env: {env}");
        assert!(!env.contains("HERDR_"), "{env}");
    }

    #[tokio::test]
    async fn a_crush_herdr_report_reaches_the_state_hub_and_is_answered() {
        let data = tempfile::tempdir().unwrap();
        let server = Arc::new(server_at(data.path()));
        let session_id = "crush-herdr-pane".to_string();
        let mut subscription = server.agent_states.subscribe(&session_id);
        let (client, server_side) = UnixStream::pair().unwrap();
        let task = tokio::spawn(Arc::clone(&server).serve_agent_state_connection(
            Arc::clone(&server.agent_states),
            server.session_service.clone(),
            crate::scoped_contracts::Epoch(server.epoch),
            server_side,
        ));
        let (read_half, mut write_half) = client.into_split();
        let mut reader = BufReader::new(read_half);
        let report = format!(
            "{{\"id\":\"crush:report:42\",\"method\":\"pane.report_agent\",\"params\":{{\"pane_id\":\"{session_id}\",\"source\":\"crush\",\"agent\":\"crush\",\"state\":\"blocked\",\"seq\":4,\"agent_session_id\":\"3df112d9-e56f-4f69-b8bb-c58b5d580e89\"}}}}\n"
        );
        write_half.write_all(report.as_bytes()).await.unwrap();

        let mut reply = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut reply))
            .await
            .expect("crush waits only 500 ms for its reply")
            .unwrap();
        let reply: serde_json::Value = serde_json::from_str(reply.trim()).expect("one JSON line");
        assert_eq!(reply["id"], "crush:report:42");

        let published = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let update = subscription.receiver.recv().await.expect("hub stays open");
                if update.state.session_id == session_id {
                    return update.state;
                }
            }
        })
        .await
        .expect("the report must reach the hub");
        assert_eq!(published.state, "blocked");
        assert_eq!(published.agent.as_deref(), Some("crush"));
        assert_eq!(
            published.provider_session.map(|provider| provider.id).as_deref(),
            Some("3df112d9-e56f-4f69-b8bb-c58b5d580e89")
        );
        drop(write_half);
        task.await.expect("connection ends when crush hangs up");
    }

    #[tokio::test]
    async fn set_agent_integration_applies_to_new_panes_without_restart() {
        let data = tempfile::tempdir().unwrap();
        let server = Arc::new(server_at(data.path()));
        let (client, server_side) = UnixStream::pair().unwrap();
        let handler = Arc::clone(&server);
        let task = tokio::spawn(async move { handler.handle_client(server_side).await });
        let (read_half, mut write_half) = client.into_split();
        let mut reader = BufReader::new(read_half);

        let resp = exchange(&mut write_half, &mut reader, DaemonRequest::GetAgentIntegration).await;
        assert!(matches!(resp, DaemonResponse::AgentIntegrationOk { enabled: true }), "{resp:?}");
        let env = pane_env(&server).await;
        assert!(
            env.contains(&format!("FERRYX_AGENT_STATE_SOCKET={}", agent_state_socket_path())),
            "{env}"
        );

        let resp = exchange(
            &mut write_half,
            &mut reader,
            DaemonRequest::SetAgentIntegration { enabled: false },
        )
        .await;
        assert!(matches!(resp, DaemonResponse::AgentIntegrationOk { enabled: false }), "{resp:?}");
        let resp = exchange(&mut write_half, &mut reader, DaemonRequest::GetAgentIntegration).await;
        assert!(matches!(resp, DaemonResponse::AgentIntegrationOk { enabled: false }), "{resp:?}");

        // Same daemon, no restart: the very next pane has no endpoint.
        let env = pane_env(&server).await;
        assert!(env.contains("FERRYX_SESSION_ID="), "the pane ran env: {env}");
        assert!(!env.contains("FERRYX_AGENT_STATE_SOCKET"), "{env}");

        // The opt-out sits beside remote-config.json and survives a restart.
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(data.path().join("agent-integration.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(saved, serde_json::json!({ "enabled": false }));
        assert!(!server_at(data.path()).agent_integration.enabled());
        task.abort();
    }
}

#[cfg(test)]
mod a05_compatibility_tests {
    use super::*;

    #[test]
    fn catalog_constructor_child() {
        let Some(root) = std::env::var_os("A05_CONSTRUCTOR_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let sentinel = root.join("data/remote/machine-workspaces.v1.json");
        let before = fs::read(&sentinel).unwrap();
        let first = DaemonServer::new();
        let second = DaemonServer::new_with_paths(None, None);
        assert!(
            first.session_service.workspace_service.catalog().is_ok(),
            "constructor read canonical sentinel"
        );
        let first_dir = first
            .session_service
            .remote_sessions_path
            .parent()
            .unwrap()
            .to_owned();
        let second_dir = second
            .session_service
            .remote_sessions_path
            .parent()
            .unwrap()
            .to_owned();
        assert_ne!(first_dir, second_dir);
        for (server, name) in [(&first, "one"), (&second, "two")] {
            let plain = root.join(name);
            fs::create_dir(&plain).unwrap();
            server
                .handle_register_workspace("same-id", plain.to_str().unwrap())
                .unwrap();
            assert_eq!(
                server.workspace_registry.repo_root("same-id").unwrap(),
                fs::canonicalize(plain).unwrap()
            );
        }
        assert_eq!(fs::read(&sentinel).unwrap(), before);
        drop(first);
        drop(second);
        assert!(!first_dir.exists());
        assert!(!second_dir.exists());
        eprintln!(
            "CONSTRUCTOR cleanup first={} second={} absent=true sentinel_unchanged=true",
            first_dir.display(),
            second_dir.display()
        );
    }

    #[tokio::test]
    async fn catalog_constructor_isolation() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("data/remote")).unwrap();
        fs::write(
            root.path().join("data/remote/machine-workspaces.v1.json"),
            b"private canonical sentinel",
        )
        .unwrap();
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "daemon::server::a05_compatibility_tests::catalog_constructor_child",
                "--nocapture",
            ])
            .env("A05_CONSTRUCTOR_ROOT", root.path())
            .env("FERRYX_DATA_DIR", root.path().join("data"))
            .env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
            .env("HOME", root.path())
            .env("TMPDIR", root.path())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let status = match tokio::time::timeout(Duration::from_secs(20), child.wait()).await {
            Ok(status) => status.unwrap(),
            Err(_) => {
                child.start_kill().unwrap();
                child.wait().await.unwrap()
            }
        };
        let receipt = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "CONSTRUCTOR owner pid={pid} reaped=true root={} absent={}",
            receipt.display(),
            !receipt.exists()
        );
        assert!(status.success());
    }

    #[tokio::test]
    async fn catalog_ssh_unregister_compatibility() {
        let root = tempfile::tempdir().unwrap();
        let server = DaemonServer::new_with_paths(
            Some(root.path().join("data/config")),
            Some(root.path().join("data/auth")),
        );
        let id = "ssh:isolated-fixture";
        let metadata = serde_json::from_value(serde_json::json!({
            "client_request_id": "fixture", "workspace_id": id, "worktree": null,
            "cwd": root.path(), "provider_claim": null,
            "spawn_fingerprint": {"workspace_id": id, "worktree": null, "cwd": null,
                "cols": 80, "rows": 24, "shell": null, "provider_claim": null, "startup": null}
        }))
        .unwrap();
        server
            .session_metadata
            .write()
            .insert("expired-ssh-session".into(), metadata);
        let result = server.handle_unregister_workspace(id).await;
        let cleaned = !server
            .session_metadata
            .read()
            .contains_key("expired-ssh-session");
        assert!(server
            .handle_unregister_workspace("daemon:desktop")
            .await
            .is_err());
        assert!(!root.path().join("data/machine-workspaces.v1.json").exists());
        drop(server);
        let receipt = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "SSH cleanup root={} absent={} no_hosts_contacted=true ownership_released={cleaned}",
            receipt.display(),
            !receipt.exists()
        );
        assert!(result.is_ok(), "SSH unregister rejected: {result:?}");
        assert!(cleaned);
    }
}

#[cfg(all(test, unix))]
#[path = "../paired_host/process_tests.rs"]
mod paired_host_process_tests;

#[cfg(all(test, unix))]
#[path = "../paired_host/native_operation_tests.rs"]
mod paired_host_operation_tests;

#[cfg(all(test, unix))]
mod paired_host_native_tests {
    use super::*;
    use crate::paired_host::{
        inventory::{AuthStatus, MigrationReceipt},
        service::{PairRequest, Secret},
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_daemon_relay_inventory_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(60), exercise(root.path())).await;
        let path = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "A13 cleanup private_root={} absent={}",
            path.display(),
            !path.exists()
        );
        outcome.unwrap().unwrap();
    }
    struct GatewayGuard(Arc<DaemonServer>);
    impl Drop for GatewayGuard {
        fn drop(&mut self) {
            if let Some(handle) = self.0.remote_server_handle.lock().take() {
                handle.stop();
            }
        }
    }
    async fn exercise(root: &Path) -> anyhow::Result<()> {
        use anyhow::{ensure, Context};
        let data = root.join("data");
        fs::create_dir_all(&data)?;
        let mut daemon =
            DaemonServer::new_with_paths(Some(data.join("config")), Some(data.join("auth")));
        daemon.paired_hosts =
            crate::paired_host::service::PairedHostService::open_test_loopback(data.clone());
        let daemon = Arc::new(daemon);
        let _gateway_guard = GatewayGuard(Arc::clone(&daemon));
        let socket = root.join("native.sock");
        let listener = tokio::net::UnixListener::bind(&socket)?;
        let owner = Arc::clone(&daemon);
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let owner = Arc::clone(&owner);
                        clients.spawn(async move { owner.handle_client(stream).await });
                    }
                    Err(error) => return Err(error),
                }
            }
        });
        let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
            vec![],
            root.join("relay-keys.json"),
        )
        .map_err(anyhow::Error::msg)?;
        let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", relay_listener.local_addr()?);
        tasks.spawn(async move {
            axum::serve(
                relay_listener,
                crate::remote::relay_server::relay_router(relay_state)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
        });
        let result = async {
            daemon.configure_gateway(RemoteGatewayConfig { mode: RemoteNetworkMode::Relay, port: 0,
                relay_url: Some(origin.clone()), ..Default::default() }).await.map_err(anyhow::Error::msg)?;
            let client = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
            ensure!(client.paired_host_list().await.map_err(|e| anyhow::anyhow!(e.code))?.is_empty(), "new inventory not empty");
            let coordinator = daemon.remote_state.relay_pairing.read().as_ref()
                .map(|p| p.coordinator.clone()).context("published relay coordinator")?;
            let issue = |scope| {
                let coordinator = coordinator.clone();
                async move {
                    // The existing issuer does not receive relay-consumed notifications.
                    // Explicitly expire its completed PIN before the next issuance.
                    if coordinator.state() == crate::remote::protocol::PairingState::Ready {
                        coordinator.transition(crate::remote::protocol::PairingState::Expired)?;
                    }
                    coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control, scope).await
                }
            };
            let first = issue(crate::remote::auth::DeviceAccessScope::Machine).await.map_err(anyhow::Error::msg)?;
            let host = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(first.pin), display_label: "Fixture host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(host.generation.0 == 1 && host.auth_status == AuthStatus::Paired, "first adoption mismatch");
            // An independent gateway on the same relay establishes real credential isolation.
            let other_data = root.join("other");
            fs::create_dir_all(&other_data)?;
            let other = Arc::new(DaemonServer::new_with_paths(Some(other_data.join("config")), Some(other_data.join("auth"))));
            let _other_guard = GatewayGuard(Arc::clone(&other));
            other.configure_gateway(RemoteGatewayConfig { mode: RemoteNetworkMode::Relay, port: 0,
                relay_url: Some(origin.clone()), ..Default::default() }).await.map_err(anyhow::Error::msg)?;
            let other_coordinator = other.remote_state.relay_pairing.read().as_ref().map(|p| p.coordinator.clone()).context("other coordinator")?;
            let other_pin = other_coordinator.generate_scoped_pairing(Duration::from_secs(60), DevicePermission::Control,
                crate::remote::auth::DeviceAccessScope::Machine).await.map_err(anyhow::Error::msg)?;
            let other_host = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(other_pin.pin), display_label: "Other host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(other_host.host_id != host.host_id, "gateway identities aliased");
            let other_lease = daemon.paired_hosts.test_capture(other_host.host_id.clone(), other_host.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            client.paired_host_forget(other_host.host_id.clone(), other_host.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(other_lease.token().is_err(), "other forget did not cancel its lease");
            ensure!(client.paired_host_read(MigrationReceipt { host_id: host.host_id.clone(), generation: host.generation }).await.map_err(|e| anyhow::anyhow!(e.code))? == host, "cross-host forget mutated first host");
            ensure!(!other.remote_state.auth_manager.list_devices().is_empty(), "other forget revoked remote grant");
            let lease = daemon.paired_hosts.test_capture(host.host_id.clone(), host.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            let mut cancelled = lease.cancellation();
            let token = lease.token()?.to_owned();
            let migrated = client.paired_host_migrate_legacy(crate::paired_host::service::MigrationRequest {
                relay_origin: origin.clone(), machine_id: host.machine_id.clone(), display_label: host.display_label.clone(), device_token: Secret(token.clone()),
            }).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(migrated.generation == host.generation && migrated.host_id == host.host_id, "migration retry changed authority");
            ensure!(client.paired_host_read(migrated).await.map_err(|e| anyhow::anyhow!(e.code))? == host, "migration durable receipt failed");
            let serialized = serde_json::to_string(&host)?;
            ensure!(!serialized.contains(&token) && !serialized.contains("deviceToken"), "native projection leaked credential");
            use std::os::unix::fs::PermissionsExt;
            ensure!(fs::metadata(data.join("paired-hosts.v1.json"))?.permissions().mode() & 0o777 == 0o600, "credential file not private");
            ensure!(fs::metadata(&data)?.permissions().mode() & 0o777 == 0o700, "credential directory not private");
            let second = issue(crate::remote::auth::DeviceAccessScope::Machine).await.map_err(anyhow::Error::msg)?;
            let repaired = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(second.pin), display_label: "Fixture host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(repaired.generation.0 == 2 && repaired.host_id == host.host_id, "re-pair identity/generation mismatch");
            cancelled.changed().await?;
            ensure!(*cancelled.borrow() && lease.token().is_err(), "lease not cancelled");
            ensure!(client.paired_host_forget(host.host_id.clone(), host.generation).await.unwrap_err().code == "PAIRED_HOST_STALE_GENERATION", "stale forget admitted");
            let receipt = MigrationReceipt { host_id: repaired.host_id.clone(), generation: repaired.generation };
            ensure!(client.paired_host_read(receipt).await.map_err(|e| anyhow::anyhow!(e.code))? == repaired, "durable readback mismatch");
            let restart = crate::paired_host::service::PairedHostService::open_test_loopback(data.clone());
            // A restarted service has probed nothing yet, so it truthfully reports the host not online.
            let restarted = crate::paired_host::inventory::HostView { online: false, ..repaired.clone() };
            ensure!(restart.list().await.map_err(|e| anyhow::anyhow!(e.code))? == vec![restarted], "restart persistence mismatch");
            drop(restart);
            let migration = crate::paired_host::service::MigrationRequest { relay_origin: origin.clone(), machine_id: repaired.machine_id.clone(), display_label: repaired.display_label.clone(), device_token: Secret(token.clone()) };
            ensure!(client.paired_host_migrate_legacy(migration).await.unwrap_err().code == "PAIRED_HOST_MIGRATION_PENDING", "old credential replaced re-pair");
            ensure!(client.paired_host_list().await.map_err(|e| anyhow::anyhow!(e.code))? == vec![repaired.clone()], "failed migration damaged inventory");
            let mirror = issue(crate::remote::auth::DeviceAccessScope::Mirror).await.map_err(anyhow::Error::msg)?;
            let mirrored = client.paired_host_pair(PairRequest { relay_origin: origin.clone(), pin: Secret(mirror.pin), display_label: "Fixture host".into() })
                .await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(mirrored.generation.0 == 3 && mirrored.auth_status == AuthStatus::NeedsMachineGrant, "mirror elevated");
            client.paired_host_forget(mirrored.host_id, mirrored.generation).await.map_err(|e| anyhow::anyhow!(e.code))?;
            ensure!(client.paired_host_list().await.map_err(|e| anyhow::anyhow!(e.code))?.is_empty(), "forget retained local host");
            ensure!(!daemon.remote_state.auth_manager.list_devices().is_empty(), "forget revoked remote devices");
            eprintln!("A13 real_uds=true relay_pair_exchange=true scope_negotiation=true re_pair=1,2,3 stale_forget=rejected lease_cancelled=true durable_readback=true restart=true failed_migration_preserved=true local_forget=true cross_host_isolation=true modes=700,600");
            Ok::<_, anyhow::Error>(())
        }.await;
        if let Some(handle) = daemon.remote_server_handle.lock().take() {
            handle.stop();
        }
        tasks.shutdown().await;
        drop(daemon);
        fs::remove_file(socket)?;
        result
    }
}

#[cfg(all(test, unix))]
#[path = "a03_owner_cli_fixture.rs"]
mod a03_owner_cli_fixture;

/// Todo 11: live identity of a paired session across a client disconnect/reconnect (the
/// daemon-side equivalent of GUI quit/relaunch), and the uncertain-write fixture proving an
/// unacknowledged write is applied exactly once and never resent.
#[cfg(all(test, unix))]
mod live_inventory_paired_tests {
    use super::*;
    use axum::{
        extract::ws::{Message, WebSocketUpgrade},
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;

    const REMOTE_PID: u32 = 4242;

    /// A paired host on loopback. Every binary frame (terminal input) it receives is appended
    /// to `received`, so the test reads the host's exact received-byte count.
    struct MockHost {
        origin: String,
        received: tokio::sync::watch::Receiver<Vec<u8>>,
        shutdown: Option<tokio::sync::oneshot::Sender<()>>,
        server: tokio::task::JoinHandle<()>,
    }

    async fn mock_host() -> MockHost {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (received_tx, received) = tokio::sync::watch::channel(Vec::<u8>::new());
        let received_tx = Arc::new(received_tx);
        let session = || json!({
            "target": {"machineId": "a", "daemonEpoch": "1", "sessionId": "s"},
            "workspaceId": "w", "worktree": null, "cwd": "/fixture", "cols": 80, "rows": 24,
            "running": true, "providerSession": null, "startSequence": "10", "endSequence": "10",
            "pid": REMOTE_PID
        });
        let router = Router::new()
            .route("/api/v1/pair/exchange", post(|| async {
                Json(json!({"token": "fixture-secret", "machineId": "a", "device": {"id": "d", "name": "d",
                    "permission": "control", "accessScope": "machine", "createdAt": 1, "lastSeenAt": 1}}))
            }))
            .route("/host/a/api/v1/capabilities", get(|| async {
                Json(json!({"apiVersion": 1, "machineId": "a", "daemonEpoch": "1", "platform": "linux",
                    "accessScope": "machine", "permission": "control",
                    "capabilities": ["terminalCreateV1", "terminalStreamV1"],
                    "limits": {"directoryEntries": 1000, "terminalSessions": 64}}))
            }))
            .route("/host/a/api/v1/sessions/s", get(move || async move {
                Json(json!({"status": "running", "session": session()}))
            }))
            .route("/host/a/api/v1/socket-ticket", post(|| async { Json(json!({"ticket": "fixture-ticket"})) }))
            .route("/host/a/api/v1/terminal/s", get(move |ws: WebSocketUpgrade| {
                let received_tx = Arc::clone(&received_tx);
                async move {
                    ws.on_upgrade(move |mut socket| async move {
                        let attached = json!({"type": "attached",
                            "target": {"machineId": "a", "daemonEpoch": "1", "sessionId": "s"},
                            "generation": "7", "cols": 80, "rows": 24, "startSequence": "10",
                            "endSequence": "10", "replayGap": null});
                        if socket.send(Message::Text(attached.to_string().into())).await.is_err() {
                            return;
                        }
                        while let Some(message) = socket.recv().await {
                            match message {
                                Ok(Message::Binary(bytes)) => {
                                    received_tx.send_modify(|all| all.extend_from_slice(&bytes));
                                }
                                Ok(Message::Close(_)) | Err(_) => break,
                                Ok(_) => {}
                            }
                        }
                    })
                }
            }));
        let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async { let _ = stopped.await; })
                .await
                .unwrap();
        });
        MockHost { origin: format!("http://{address}"), received, shutdown: Some(shutdown), server }
    }

    /// A daemon serving UDS clients with a loopback paired-host inventory paired to `host`,
    /// plus a descriptor for the host's session "s".
    async fn daemon_with_paired_host(
        root: &Path,
        host: &MockHost,
    ) -> (Arc<DaemonServer>, PathBuf, crate::terminal::paired_daemon::Descriptor, tokio::task::JoinHandle<()>) {
        // root stands in for the daemon runtime directory: legacy handover sockets live there,
        // and LegacyPeer refuses a socket whose directory is not 0700 (validate_runtime_socket_path),
        // which would make the successor drop the predecessor's route as unreachable.
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        let mut daemon = DaemonServer::new_with_paths(Some(data.join("config")), Some(data.join("auth")));
        daemon.paired_hosts = crate::paired_host::service::PairedHostService::open_test_loopback(data.join("paired"));
        let view = daemon.paired_hosts.pair(crate::paired_host::service::PairRequest {
            relay_origin: host.origin.clone(),
            pin: crate::paired_host::service::Secret("fixture".into()),
            display_label: "fixture".into(),
        }).await.unwrap();
        let descriptor = crate::terminal::paired_daemon::Descriptor {
            host_id: view.host_id,
            generation: view.generation,
            target: crate::remote::machine_protocol::RemoteTerminalTarget {
                machine_id: "a".into(),
                daemon_epoch: crate::scoped_contracts::Epoch(1),
                session_id: "s".into(),
            },
            after_sequence: None,
        };
        let daemon = Arc::new(daemon);
        let socket = root.join("live.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let owner = Arc::clone(&daemon);
        let accept = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let owner = Arc::clone(&owner);
                tokio::spawn(async move { owner.handle_client(stream).await });
            }
        });
        (daemon, socket, descriptor, accept)
    }

    async fn live_entry(socket: &Path, id: &str) -> crate::daemon::protocol::LiveSessionInfo {
        let client = crate::daemon::client::DaemonClient::new_with_socket(socket.to_owned());
        let sessions = tokio::time::timeout(Duration::from_secs(10), client.describe_live_sessions())
            .await
            .expect("describe_live_sessions timed out")
            .expect("describe_live_sessions failed");
        sessions.into_iter().find(|s| s.backend_id == id).expect("paired session missing from inventory")
    }

    /// Reattach answers as soon as the proxy actor is installed (with a placeholder generation);
    /// the actor attaches to the host in the background. Wait for that attach so the live
    /// identity the tests assert on (remote pid, connected, controller generation) is the
    /// host's and not the pre-attach state.
    async fn reattach_attached(
        daemon: &DaemonServer,
        client: &crate::daemon::client::DaemonClient,
        descriptor: crate::terminal::paired_daemon::Descriptor,
    ) -> (String, crate::scoped_contracts::Epoch) {
        let (id, _placeholder) = client.paired_terminal_reattach(descriptor).await.unwrap();
        let mut outcome = daemon
            .terminal_service
            .paired()
            .subscribe_recovery_outcome(&id)
            .expect("reattach installs a proxy actor");
        let attached = tokio::time::timeout(
            Duration::from_secs(10),
            outcome.wait_for(|outcome| outcome.state == "proxy_attached"),
        )
        .await
        .expect("paired proxy did not attach to the host")
        .expect("paired proxy actor ended before attaching")
        .clone();
        let generation = attached.generation.expect("an attached proxy holds a controller generation");
        (id, crate::scoped_contracts::Epoch(generation))
    }

    async fn teardown(mut host: MockHost, accept: tokio::task::JoinHandle<()>, daemon: Arc<DaemonServer>, id: &str) {
        let _ = daemon.terminal_service.paired().detach(id).await;
        accept.abort();
        let _ = accept.await;
        if let Some(shutdown) = host.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = host.server.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paired_session_keeps_backend_id_and_remote_pid_across_client_reconnect() {
        let root = tempfile::tempdir().unwrap();
        let host = mock_host().await;
        let (daemon, socket, descriptor, accept) = daemon_with_paired_host(root.path(), &host).await;

        // First client attaches, observes the session, then disconnects (GUI quit).
        let first = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
        let (id, _generation) = reattach_attached(&daemon, &first, descriptor).await;
        let before = live_entry(&socket, &id).await;
        drop(first);

        // A fresh client (GUI relaunch) sees the same live identity.
        let after = live_entry(&socket, &id).await;
        assert_eq!(before.kind, crate::daemon::protocol::LiveSessionKind::Paired);
        assert_eq!(after.backend_id, before.backend_id);
        assert_eq!(before.remote_pid, Some(REMOTE_PID));
        assert_eq!(after.remote_pid, before.remote_pid);
        assert!(after.connected && after.reader_open, "{after:?}");
        assert_eq!(after.local_child_pid, None, "a remote pid is never a local pid");
        assert_eq!(after.local_child_start_time, None);

        teardown(host, accept, daemon, &id).await;
    }

    /// Makes daemon a committed predecessor that kept its paired session: prepared on a
    /// legacy socket inside root, then committed through the real v5 commit.
    async fn commit_predecessor_keeping_retained(daemon: &Arc<DaemonServer>, root: &Path) -> PathBuf {
        let legacy = root.join("legacy-a.sock");
        let listener = tokio::net::UnixListener::bind(&legacy).unwrap();
        Arc::clone(daemon).serve_legacy_handover_socket(listener);
        daemon.handover_manager.force_prepared_for_test(legacy.clone());
        let (retired_tx, _retired_rx) = std::sync::mpsc::channel::<()>();
        let retired_tx = std::sync::Mutex::new(retired_tx);
        daemon.handover_manager.set_retirement_action(move || {
            let _ = retired_tx.lock().unwrap().send(());
        });
        let manager = Arc::clone(&daemon.handover_manager);
        let service = Arc::clone(&daemon.terminal_service);
        tokio::task::spawn_blocking(move || manager.commit_handover_v5(&service))
            .await
            .unwrap()
            .expect("commit with a retained paired session");
        legacy
    }

    /// GAP-10: a successful handover leaves a paired session owned and served by the
    /// predecessor (Draining); the successor routes to it directly and creates no controller.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn committed_predecessor_keeps_serving_its_paired_session_through_the_successor() {
        let root = tempfile::tempdir().unwrap();
        let host = mock_host().await;
        let (daemon, socket, descriptor, accept) = daemon_with_paired_host(root.path(), &host).await;
        let client = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
        let (id, generation) = reattach_attached(&daemon, &client, descriptor).await;
        let before = live_entry(&socket, &id).await;

        let durable = root.path().join("remote_sessions.json");
        daemon.persist_remote_sessions_at(durable.clone()).await.unwrap();
        let legacy = commit_predecessor_keeping_retained(&daemon, root.path()).await;
        assert_eq!(daemon.handover_manager.status(), crate::daemon::handover::HandoverStatus::Draining);
        assert_eq!(
            crate::daemon::handover::HandoverManager::recorded_decision(&legacy).unwrap(),
            Some(crate::daemon::handover_transaction::HandoverState::Retired),
            "the durable decision stays Retired while the process drains"
        );
        assert!(!daemon.handover_manager.accept_gate_open(), "public listeners accept nothing after commit");
        let manifest = crate::daemon::manifest::HandoverManifest::load_from_path(&root.path().join("handover_routes.json"));
        assert!(manifest.routes.iter().any(|r| r.legacy_socket_path == legacy && r.sessions.contains(&id)));

        // The successor adopts the route and restores nothing for the retained session.
        let successor = Arc::new(DaemonServer::new_with_paths(
            Some(root.path().join("succ/config")),
            Some(root.path().join("succ/auth")),
        ));
        successor
            .session_router
            .adopt_routes_from_manifest_at(root.path().join("handover_routes.json"))
            .await
            .unwrap();
        // The successor's real restore path, run after the route adoption exactly as the
        // committed candidate does, creates no controller for the retained session.
        successor.restore_remote_sessions_at(durable).await.unwrap();
        assert!(successor.terminal_service.remote().list().is_empty(), "successor created an SSH controller");
        assert!(successor.terminal_service.paired().list().is_empty(), "successor created a controller");
        let successor_socket = root.path().join("successor.sock");
        let successor_listener = tokio::net::UnixListener::bind(&successor_socket).unwrap();
        let serving = Arc::clone(&successor);
        let successor_accept = tokio::spawn(async move {
            while let Ok((stream, _)) = successor_listener.accept().await {
                let serving = Arc::clone(&serving);
                tokio::spawn(async move { serving.handle_client(stream).await });
            }
        });

        // Input through the successor's socket reaches the predecessor's actor and the host.
        let mut received = host.received.clone();
        let baseline = received.borrow().len();
        let marker = b"todo8-retained\n".to_vec();
        let through_successor = crate::daemon::client::DaemonClient::new_with_socket(successor_socket.clone());
        tokio::time::timeout(Duration::from_secs(10), through_successor.write_terminal_at_generation(&id, Some(generation.0), marker.clone()))
            .await
            .expect("routed write timed out")
            .expect("routed write accepted");
        tokio::time::timeout(Duration::from_secs(10), received.wait_for(|all| all.len() >= baseline + marker.len()))
            .await
            .expect("input never reached the host")
            .unwrap();
        // Same backend_session_id and remote_pid, still served by the predecessor.
        let after = live_entry(&socket, &id).await;
        assert_eq!(after.backend_id, before.backend_id);
        assert_eq!(after.remote_pid, before.remote_pid);
        assert!(daemon.terminal_service.paired().contains(&id));

        successor_accept.abort();
        teardown(host, accept, daemon, &id).await;
    }

    /// After commit a request for a transferred local session returns SESSION_NOT_OWNED on the
    /// legacy socket, while the retained paired session keeps being served there.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transferred_local_answers_session_not_owned_and_retained_session_is_served() {
        let root = tempfile::tempdir().unwrap();
        let host = mock_host().await;
        let (daemon, socket, descriptor, accept) = daemon_with_paired_host(root.path(), &host).await;
        let client = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
        let (id, _generation) = reattach_attached(&daemon, &client, descriptor).await;
        daemon.handover_manager.note_transferred_local("local-moved");
        let legacy = commit_predecessor_keeping_retained(&daemon, root.path()).await;

        let on_legacy = crate::daemon::client::DaemonClient::new_with_socket(legacy.clone());
        let moved = on_legacy
            .send_request(DaemonRequest::Write { session_id: "local-moved".into(), data: b"x".to_vec() })
            .await;
        match moved {
            Ok(DaemonResponse::Error { code, .. }) => {
                assert_eq!(code.as_deref(), Some(crate::daemon::handover::SESSION_NOT_OWNED));
            }
            other => panic!("expected SESSION_NOT_OWNED, got {other:?}"),
        }
        let retained = live_entry(&legacy, &id).await;
        assert!(retained.connected, "{retained:?}");
        teardown(host, accept, daemon, &id).await;
    }

    /// A committed predecessor that keeps a paired session (Draining) stops its paired-host
    /// probe loop, so it never revokes or rewrites the inventory its successor now owns.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn draining_predecessor_stops_its_paired_host_probe_loop() {
        let root = tempfile::tempdir().unwrap();
        let host = mock_host().await;
        let (daemon, socket, descriptor, accept) = daemon_with_paired_host(root.path(), &host).await;
        let client = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
        let (id, _generation) = reattach_attached(&daemon, &client, descriptor).await;
        let probe = daemon.spawn_paired_host_probe(Duration::from_millis(20));
        // The loop exits only once its predicate reads false, so while it reads true the
        // loop is still running and the bounded wait below measures the commit alone.
        assert!(DaemonServer::owns_paired_host_inventory(&Arc::downgrade(&daemon)));
        assert!(!probe.is_finished());
        commit_predecessor_keeping_retained(&daemon, root.path()).await;
        assert_eq!(daemon.handover_manager.status(), crate::daemon::handover::HandoverStatus::Draining);
        assert!(!DaemonServer::owns_paired_host_inventory(&Arc::downgrade(&daemon)));
        // The test and the accept task still hold the server, so only the handover state
        // can end the loop.
        tokio::time::timeout(Duration::from_secs(5), probe)
            .await
            .expect("the probe loop must stop once this daemon is draining")
            .unwrap();
        teardown(host, accept, daemon, &id).await;
    }

    /// Chained upgrade A -> B -> C: routes are direct. A keeps its session, B commits holding
    /// nothing of its own and retires (exit) without waiting for A, C serves A's session through
    /// A's own route, and A retires only after the session closes and its route is removed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn chained_upgrade_routes_directly_to_the_generation_that_owns_the_session() {
        let root = tempfile::tempdir().unwrap();
        let host = mock_host().await;
        let (a, socket_a, descriptor, accept) = daemon_with_paired_host(root.path(), &host).await;
        let client = crate::daemon::client::DaemonClient::new_with_socket(socket_a.clone());
        let (id, generation) = reattach_attached(&a, &client, descriptor).await;
        let remote_pid = live_entry(&socket_a, &id).await.remote_pid;
        let (a_retired_tx, mut a_retired) = tokio::sync::watch::channel(false);
        let legacy_a = root.path().join("legacy-a.sock");
        Arc::clone(&a).serve_legacy_handover_socket(tokio::net::UnixListener::bind(&legacy_a).unwrap());
        a.handover_manager.force_prepared_for_test(legacy_a.clone());
        a.handover_manager.set_retirement_action(move || {
            a_retired_tx.send_replace(true);
        });
        {
            let manager = Arc::clone(&a.handover_manager);
            let service = Arc::clone(&a.terminal_service);
            tokio::task::spawn_blocking(move || manager.commit_handover_v5(&service)).await.unwrap().unwrap();
        }
        let manifest_path = root.path().join("handover_routes.json");

        // B: adopted A's route, owns nothing itself.
        let b = Arc::new(DaemonServer::new_with_paths(Some(root.path().join("b/config")), Some(root.path().join("b/auth"))));
        b.session_router.adopt_routes_from_manifest_at(manifest_path.clone()).await.unwrap();
        let legacy_b = root.path().join("legacy-b.sock");
        b.handover_manager.force_prepared_for_test(legacy_b.clone());
        let (b_retired_tx, b_retired) = std::sync::mpsc::channel::<()>();
        let b_retired_tx = std::sync::Mutex::new(b_retired_tx);
        b.handover_manager.set_retirement_action(move || {
            let _ = b_retired_tx.lock().unwrap().send(());
        });
        {
            let manager = Arc::clone(&b.handover_manager);
            let service = Arc::clone(&b.terminal_service);
            tokio::task::spawn_blocking(move || manager.commit_handover_v5(&service)).await.unwrap().unwrap();
        }
        // B counts only its own sessions (none): it retires although A still serves one.
        assert_eq!(b.handover_manager.status(), crate::daemon::handover::HandoverStatus::Retired);
        tokio::task::spawn_blocking(move || b_retired.recv_timeout(Duration::from_secs(10)))
            .await
            .unwrap()
            .expect("B exits 0 after its commit");
        let manifest = crate::daemon::manifest::HandoverManifest::load_from_path(&manifest_path);
        assert!(manifest.routes.iter().all(|r| r.legacy_socket_path != legacy_b), "B wrote a route it does not need");
        assert!(manifest.routes.iter().any(|r| r.legacy_socket_path == legacy_a), "A's route was disturbed");

        // C: talks to A directly.
        let c = Arc::new(DaemonServer::new_with_paths(Some(root.path().join("c/config")), Some(root.path().join("c/auth"))));
        c.session_router.adopt_routes_from_manifest_at(manifest_path.clone()).await.unwrap();
        let peer = c.session_router.find_legacy_peer_for_session(&id).expect("C routes A's session");
        assert_eq!(peer.socket_path(), legacy_a.as_path());
        assert!(c.terminal_service.paired().list().is_empty());
        let mut received = host.received.clone();
        let baseline = received.borrow().len();
        match peer.send_request(&DaemonRequest::RemoteWrite { session_id: id.clone(), generation: generation.0, data: b"chained\n".to_vec() }).await {
            Ok(DaemonResponse::WriteOk) => {}
            other => panic!("routed write failed: {other:?}"),
        }
        tokio::time::timeout(Duration::from_secs(10), received.wait_for(|all| all.len() >= baseline + 8))
            .await
            .expect("chained input never reached the host")
            .unwrap();
        assert_eq!(live_entry(&legacy_a, &id).await.remote_pid, remote_pid);
        assert!(!*a_retired.borrow(), "A must stay while its session is open");

        // Closing A's last session retires A and removes its route.
        a.terminal_service.paired().detach(&id).await.unwrap();
        a.handover_manager.check_retirement_if_empty(&a.terminal_service);
        tokio::time::timeout(Duration::from_secs(10), a_retired.wait_for(|done| *done))
            .await
            .expect("A never retired")
            .unwrap();
        let manifest = crate::daemon::manifest::HandoverManifest::load_from_path(&manifest_path);
        assert!(manifest.routes.iter().all(|r| r.legacy_socket_path != legacy_a), "A's route outlived A");
        teardown(host, accept, a, &id).await;
    }

    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn held_write_ack_applies_marker_exactly_once_without_resend() {
        let root = tempfile::tempdir().unwrap();
        let host = mock_host().await;
        let (daemon, socket, descriptor, accept) = daemon_with_paired_host(root.path(), &host).await;
        let attach = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
        let (id, generation) = reattach_attached(&daemon, &attach, descriptor).await;
        drop(attach);
        let before = live_entry(&socket, &id).await;
        let mut received = host.received.clone();
        let baseline = received.borrow().len();

        let hold = crate::ipc::qa_barrier::write_ack_hold();
        hold.arm(&id);
        let mut applied = hold.subscribe_applied();
        let marker = format!("todo11-marker-{}\n", uuid::Uuid::new_v4()).into_bytes();

        // The writer's acknowledgement is held: it never returns on its own.
        let writer = {
            let client = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
            let (id, marker) = (id.clone(), marker.clone());
            tokio::spawn(async move { client.write_terminal_at_generation(&id, Some(generation.0), marker).await })
        };
        loop {
            let event = tokio::time::timeout(Duration::from_secs(10), applied.recv())
                .await
                .expect("write-applied never emitted")
                .unwrap();
            if event.session_id == id {
                break;
            }
        }
        tokio::time::timeout(Duration::from_secs(10), received.wait_for(|all| all.len() >= baseline + marker.len()))
            .await
            .expect("marker never reached the host")
            .unwrap();
        assert!(!writer.is_finished(), "WriteOk must stay held while armed");

        // Disconnect before the acknowledgement is delivered.
        writer.abort();
        assert!(writer.await.unwrap_err().is_cancelled());
        assert!(hold.release(&id), "connection close leaves the hold armed until released");

        // Reconnect: same identity, and the next write is the next thing the host sees, so a
        // resend of the marker would land between them.
        let after = live_entry(&socket, &id).await;
        assert_eq!(after.backend_id, before.backend_id);
        assert_eq!(after.remote_pid, before.remote_pid);
        assert!(after.connected, "{after:?}");
        let sentinel = b"todo11-sentinel\n".to_vec();
        let next = crate::daemon::client::DaemonClient::new_with_socket(socket.clone());
        tokio::time::timeout(Duration::from_secs(10), next.write_terminal_at_generation(&id, Some(generation.0), sentinel.clone()))
            .await
            .expect("sentinel write timed out")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), received.wait_for(|all| all.len() >= baseline + marker.len() + sentinel.len()))
            .await
            .expect("sentinel never reached the host")
            .unwrap();

        let all = received.borrow().clone();
        let delivered = &all[baseline..];
        let occurrences = delivered.windows(marker.len()).filter(|w| *w == marker.as_slice()).count();
        assert_eq!(occurrences, 1, "marker must reach the remote exactly once");
        assert_eq!(delivered.len(), marker.len() + sentinel.len(), "host received bytes grew by exactly the marker (+ sentinel): no resend");
        assert_eq!(delivered, [marker.as_slice(), sentinel.as_slice()].concat().as_slice());

        teardown(host, accept, daemon, &id).await;
    }
}

#[cfg(all(test, unix))]
#[path = "a04_shared_services_tests.rs"]
mod a04_shared_services_tests;

impl std::ops::Deref for DaemonServer {
    type Target = DaemonSessionService;
    fn deref(&self) -> &Self::Target {
        &self.session_service
    }
}

impl Default for DaemonServer {
    fn default() -> Self {
        Self::new()
    }
}

fn daemon_error(message: impl ToString) -> DaemonResponse {
    DaemonResponse::Error {
        message: message.to_string(),
        code: None,
        details: None,
    }
}

/// Sends TRANSFER_DONE to the handover candidate. A failure is logged and recorded in failures;
/// returns whether the frame was sent.
#[cfg(unix)]
fn send_transfer_done_recorded(
    stream: &std::os::unix::net::UnixStream,
    transfer_id: &str,
    seq: u64,
    failures: &mut Vec<String>,
) -> bool {
    match crate::daemon::handover_socket::send_transfer_done(stream, transfer_id, seq) {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(
                %error,
                "Failed to send TRANSFER_DONE to the successor; the transfer is incomplete and this daemon keeps serving"
            );
            failures.push(format!("transfer done not sent: {error}"));
            false
        }
    }
}

/// The TransferSessions answer. Without a delivered TRANSFER_DONE the transfer is not complete,
/// so it is an error rather than TransferSessionsOk; the candidate then aborts and the
/// predecessor, which never stopped serving, keeps every session.
#[cfg(unix)]
fn transfer_sessions_reply(done: bool, count: usize, requested: usize, failures: &[String]) -> DaemonResponse {
    if !done {
        return daemon_error(format!(
            "Session transfer incomplete ({count} of {requested} sent): {}",
            failures.join("; ")
        ));
    }
    tracing::info!(
        requested,
        transferred = count,
        "Transferred sessions to the successor over the handover socket"
    );
    DaemonResponse::TransferSessionsOk {
        transferred_count: count,
        requested_count: requested,
    }
}

#[cfg(all(test, unix))]
mod transfer_done_tests {
    use super::*;

    #[test]
    fn a_failed_transfer_done_is_recorded_and_the_reply_is_an_error() {
        let (stream, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        drop(peer);
        let mut failures = Vec::new();
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let done = send_transfer_done_recorded(&stream, &transfer_id, 3, &mut failures);
        assert!(!done, "a closed candidate stream cannot receive TRANSFER_DONE");
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].starts_with("transfer done not sent"), "{failures:?}");
        match transfer_sessions_reply(done, 2, 2, &failures) {
            DaemonResponse::Error { message, .. } => assert!(message.contains("2 of 2"), "{message}"),
            other => panic!("a failed TRANSFER_DONE must not answer TransferSessionsOk: {other:?}"),
        }
    }

    #[test]
    fn a_delivered_transfer_done_answers_transfer_sessions_ok() {
        let (stream, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut failures = Vec::new();
        let transfer_id = uuid::Uuid::new_v4().to_string();
        assert!(send_transfer_done_recorded(&stream, &transfer_id, 1, &mut failures), "{failures:?}");
        assert!(failures.is_empty(), "{failures:?}");
        assert!(
            crate::daemon::handover_socket::recv_session_with_owner(&peer).unwrap().is_none(),
            "the candidate reads TRANSFER_DONE"
        );
        assert!(matches!(
            transfer_sessions_reply(true, 1, 1, &failures),
            DaemonResponse::TransferSessionsOk { transferred_count: 1, requested_count: 1 }
        ));
    }
}

/// Maps a handover failure to its typed outcome. A refusal produced by the in-flight spawn
/// gate is the retryable busy outcome -- `HandoverRejected` carrying the guard identity in
/// `reason` -- because the gate reopens as soon as that spawn's guard drops, and the caller
/// (the installer's upgrade poll, the desktop staleness probe) is expected to retry rather
/// than treat the handover as structurally failed. Every other failure stays a generic
/// error, prefixed with `context` when one is given.
fn handover_failure_response(context: Option<&str>, message: String) -> DaemonResponse {
    if crate::daemon::handover::is_spawn_gate_busy(&message) {
        return DaemonResponse::HandoverRejected { reason: message };
    }
    match context {
        Some(context) => daemon_error(format!("{context}: {message}")),
        None => daemon_error(message),
    }
}

/// Connects once to the candidate's own handover socket so its blocking accept worker returns
/// after an expiry abort (todo 8). The connect is a socket syscall, so it runs off the workers.
/// A failed connect or a failed join is returned: the accept worker may then stay parked and
/// keep this process from exiting, so the caller must report it.
#[cfg(unix)]
async fn wake_handover_accept(path: PathBuf) -> std::io::Result<()> {
    tokio::task::spawn_blocking(move || std::os::unix::net::UnixStream::connect(&path).map(drop))
        .await
        .map_err(std::io::Error::other)?
}

/// Expiry abort: wakes the parked accept worker and returns the abort reason, extended with the
/// wake failure when the wake did not happen.
#[cfg(unix)]
async fn expiry_abort_reason(handover_socket_path: &Path, reason: &str) -> String {
    match wake_handover_accept(handover_socket_path.to_path_buf()).await {
        Ok(()) => reason.to_string(),
        Err(error) => {
            tracing::warn!(
                %error,
                path = %handover_socket_path.display(),
                "Could not wake the handover accept worker after the request expired"
            );
            format!("{reason}; waking the handover accept worker at {} failed: {error}", handover_socket_path.display())
        }
    }
}

/// QA gate (local-split-qa): while the file named by FERRYX_QA_OWNER_PUBLISH_HOLD exists, every
/// owner.json publication attempt fails, so a test controls exactly how long the record stays
/// unpublished. Blocking (a stat); called only inside run_blocking.
fn owner_publish_held() -> bool {
    #[cfg(feature = "local-split-qa")]
    {
        std::env::var_os("FERRYX_QA_OWNER_PUBLISH_HOLD").is_some_and(|path| Path::new(&path).exists())
    }
    #[cfg(not(feature = "local-split-qa"))]
    {
        false
    }
}

fn daemon_session_not_found(session_id: &str, source: &'static str) -> DaemonResponse {
    DaemonResponse::Error {
        message: format!("Session '{session_id}' not found"),
        code: Some("SESSION_NOT_FOUND".to_string()),
        details: Some(serde_json::json!({
            "source": source,
            "kind": "session_not_found",
            "sessionId": session_id,
        })),
    }
}

/// The session a per-session request targets, for the SESSION_NOT_OWNED gate of a committed
/// predecessor.
fn request_session_id(request: &DaemonRequest) -> Option<&str> {
    match request {
        DaemonRequest::Write { session_id, .. }
        | DaemonRequest::Resize { session_id, .. }
        | DaemonRequest::Signal { session_id, .. }
        | DaemonRequest::Close { session_id }
        | DaemonRequest::Hibernate { session_id }
        | DaemonRequest::Suspend { session_id, .. }
        | DaemonRequest::Resume { session_id, .. }
        | DaemonRequest::DescribeSession { session_id }
        | DaemonRequest::Attach { session_id, .. }
        | DaemonRequest::RemoteWrite { session_id, .. }
        | DaemonRequest::RemoteResize { session_id, .. } => Some(session_id.as_str()),
        _ => None,
    }
}

/// Decides whether a v5 handover may commit after the successor has drained the transfer socket.
///
/// The predecessor gives up ownership of every session it offers BEFORE the commit. So a delivery
/// shortfall is not a warning: committing retires the predecessor, and each undelivered session
/// dies with the PTY master fd it still owned. Aborting instead leaves the predecessor serving all
/// of them, which is always the survivable outcome.
pub(crate) fn handover_delivery_verdict(offered: usize, accepted: usize) -> Result<(), String> {
    if accepted >= offered {
        return Ok(());
    }
    Err(format!(
        "Aborting handover: predecessor offered {offered} session(s) but delivered {accepted}. \
         Committing would terminate the {} undelivered session(s).",
        offered.saturating_sub(accepted)
    ))
}

/// Publish the saved paired hosts' origins to the gateway's CORS allowlist. A failed read
/// leaves the previous list in place.
async fn refresh_cors_saved_host_origins(
    paired_hosts: crate::paired_host::service::PairedHostService,
    remote_state: Arc<RemoteGatewayState>,
) {
    if let Ok(hosts) = paired_hosts.list().await {
        *remote_state.cors_saved_host_origins.write() =
            hosts.into_iter().map(|host| host.relay_origin).collect();
    }
}

/// Gate the commit on what the predecessor OWNED, not merely on what it managed to send.
///
/// `offered` counts only the sessions the predecessor successfully exported. A session that
/// fails to export is therefore absent from BOTH sides of an `offered`/`accepted` comparison,
/// so that check passes while the session dies with the predecessor's PTY master fd. This was
/// observed in production: `requested=20 transferred=18` followed by `offered=18 accepted=18`
/// and a commit -- two sessions lost with the check reporting success.
///
/// `owned == 0` means the peer is an older daemon that does not send the field; fall back to
/// the delivery-only check rather than aborting every handover with such a predecessor.
pub(crate) fn handover_delivery_verdict_owned(
    owned: usize,
    offered: usize,
    accepted: usize,
) -> Result<(), String> {
    handover_delivery_verdict(offered, accepted)?;
    if owned == 0 || accepted >= owned {
        return Ok(());
    }
    Err(format!(
        "Aborting handover: predecessor owned {owned} session(s) but only {accepted} survived the \
         transfer. Committing would terminate the {} session(s) it could not export.",
        owned.saturating_sub(accepted)
    ))
}

/// Sessions the predecessor listed but did not export, whose durable remote snapshot lets the
/// successor restore them after commit. An older predecessor counts these in `owned` even
/// though only local PTYs travel over the handover socket.
pub(crate) fn handover_restorable_unexported(
    listed: &[String],
    exported: &std::collections::HashSet<String>,
    durable: &std::collections::HashSet<String>,
) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut count = 0;
    for id in listed {
        if seen.insert(id) && !exported.contains(id) && durable.contains(id) {
            count += 1;
        }
    }
    count
}

/// `owned` as the verdict should see it. An older predecessor counts SSH/remote sessions in
/// `owned` although only local PTYs travel; those that the successor restores from the durable
/// snapshot are not casualties. A newer predecessor already reports local PTYs only, so nothing
/// is credited, and crediting it anyway would zero `owned` and disable the owned check.
pub(crate) fn handover_effective_owned(
    owned: usize,
    listed_unique: usize,
    restorable: usize,
) -> usize {
    if owned >= listed_unique && listed_unique > 0 {
        owned.saturating_sub(restorable)
    } else {
        owned
    }
}

impl DaemonServer {
    pub fn new() -> Self {
        // Headless CLI constructs synchronously inside its multi-thread runtime.
        // Yield that executor worker while startup waits for catalog restoration.
        if tokio::runtime::Handle::try_current().is_ok_and(|handle| {
            handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
        }) {
            return tokio::task::block_in_place(|| Self::new_with_paths(None, None));
        }
        Self::new_with_paths(None, None)
    }

    pub fn new_with_paths(config_path: Option<PathBuf>, auth_path: Option<PathBuf>) -> Self {
        // No-path unit constructors must never share or inspect owner state.
        // Explicit paths keep their persistent restart semantics.
        #[cfg(test)]
        let catalog_fixture = config_path
            .is_none()
            .then(|| tempfile::tempdir().expect("private test catalog"));
        let isolated_dir = config_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        #[cfg(test)]
        let isolated_dir =
            isolated_dir.or_else(|| catalog_fixture.as_ref().map(|dir| dir.path().to_owned()));
        let paired_hosts = crate::paired_host::service::PairedHostService::open(
            isolated_dir.clone().unwrap_or_else(|| {
                crate::remote::auth::canonical_identity_dir()
                    .expect("daemon requires a private data directory")
            }),
        );
        let agent_integration = Arc::new(crate::daemon::agent_extension::AgentIntegration::load(
            isolated_dir
                .as_ref()
                .map(|dir| dir.join(crate::daemon::agent_extension::AGENT_INTEGRATION_FILE_NAME))
                .or_else(crate::daemon::agent_extension::agent_integration_path),
        ));
        let pty_manager = Arc::new(
            PtyManager::new().with_agent_integration(Arc::clone(&agent_integration)),
        );
        let output_hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(
            Arc::clone(&pty_manager),
            Arc::clone(&output_hub),
        ));
        #[cfg(windows)]
        {
            // Windows has no supported way to observe a stop, so the suspension backend
            // proves ownership against this daemon's own PTY registry rather than
            // trusting a caller-supplied PID.
            let ownership = Arc::clone(&terminal_service);
            crate::terminal::install_ownership_verifier(move |pid, incarnation| {
                ownership.list_sessions().into_iter().any(|session_id| {
                    ownership.get_session(&session_id).is_some_and(|session| {
                        session.pid() == Some(pid)
                            && session.incarnation() == Some(incarnation)
                    })
                })
            });
        }
        let session_router = Arc::new(crate::daemon::proxy::SessionRouter::new(Arc::clone(
            &terminal_service,
        )));
        let workspace_registry = WorkspaceRegistry::new();
        let handover_manager = Arc::new(crate::daemon::handover::HandoverManager::new(
            get_socket_path(),
        ));
        let remote_event_tx = broadcast::channel::<DaemonRemoteEvent>(64).0;
        let catalog_path = isolated_dir
            .clone()
            .unwrap_or_else(|| {
                crate::remote::auth::canonical_identity_dir()
                    .expect("daemon requires a private data directory")
            })
            .join("machine-workspaces.v1.json");
        // Keep restore filesystem/Git work off executor threads, and join before
        // building the gateway or advertising readiness.
        let workspace_service = std::thread::scope(|scope| {
            let registry = workspace_registry.clone();
            scope
                .spawn(move || {
                    Arc::new(super::workspace_service::DaemonWorkspaceService::new(
                        registry,
                        catalog_path,
                    ))
                })
                .join()
                .expect("workspace catalog initialization panicked")
        });
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1);
        let session_service = Arc::new(DaemonSessionService {
            epoch,
            workspace_service,
            terminal_service: Arc::clone(&terminal_service),
            session_router: Arc::clone(&session_router),
            handover_manager: Arc::downgrade(&handover_manager),
            remote_event_tx: remote_event_tx.clone(),
            agent_states: Arc::new(AgentStateHub::default()),
            agent_watch: parking_lot::Mutex::new(None),
            spawn_idempotency_cache: Arc::new(Mutex::new(HashMap::new())),
            spawn_lock: Arc::new(tokio::sync::Mutex::new(())),
            split_admission: Mutex::new(HashMap::new()),
            machine_controllers: tokio::sync::Mutex::new(HashMap::new()),
            machine_lifecycles: Arc::new(Mutex::new(HashMap::new())),
            remote_persistence_lock: Arc::new(tokio::sync::Mutex::new(())),
            // The runtime dir (/tmp/ferryx-{uid}) is wiped on reboot, which erased every
            // persisted remote descriptor even though the host-side helper daemons survived.
            // Durable remote identity belongs next to the machine identity.
            remote_sessions_path: isolated_dir
                .clone()
                .or_else(session_dir_override)
                .unwrap_or_else(|| {
                    crate::remote::auth::canonical_identity_dir()
                        .expect("daemon requires a private data directory")
                })
                .join("remote_sessions.json"),
            ssh_store_path: isolated_dir
                .clone()
                .map(|p| p.join("ssh_hosts.json"))
                .unwrap_or_else(daemon_ssh_store_path),
            session_metadata: Arc::new(RwLock::new(HashMap::new())),
            provider_session_claims: Arc::new(Mutex::new(HashMap::new())),
            desktop_geometries: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            test_shell_command: parking_lot::Mutex::new(None),
        });
        #[cfg(test)]
        let remote_state = Arc::new(RemoteGatewayState::new_with_paths_and_service(
            Arc::clone(&session_router) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
            Arc::clone(&terminal_service),
            workspace_registry.clone(),
            config_path,
            auth_path,
        ));
        #[cfg(not(test))]
        let remote_state = if config_path.is_some() || auth_path.is_some() {
            Arc::new(RemoteGatewayState::new_with_paths_and_service(
                Arc::clone(&session_router)
                    as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
                config_path,
                auth_path,
            ))
        } else {
            Arc::new(RemoteGatewayState::new_persistent_with_service(
                Arc::clone(&session_router)
                    as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
                Arc::clone(&terminal_service),
                workspace_registry.clone(),
            ))
        };

        let remote_state = Arc::new(
            Arc::try_unwrap(remote_state)
                .unwrap_or_else(|_| unreachable!("gateway not published yet"))
                .with_machine_services(Arc::clone(&session_service)),
        );
        remote_state.agent_observers.attach(
            session_service.agent_state_hub(),
            Arc::clone(&session_service.workspace_service.machine_events),
        );
        session_service.install_agent_watch(Arc::clone(&remote_state.agent_observers));

        let (binary_path, binary_mtime_ms) = resolve_binary_identity();
        remote_state
            .daemon_epoch
            .store(epoch, std::sync::atomic::Ordering::Release);

        // The gateway runs inside this process, so desktop-directed events must
        // be relayed to the GUI over the socket; without this sink they are
        // dropped and remote-issued selections never reach the desktop.
        let sink_tx = remote_event_tx.clone();
        remote_state.set_desktop_event_sink(Arc::new(move |event, payload| {
            let _ = sink_tx.send(DaemonRemoteEvent {
                event: event.to_string(),
                payload,
            });
        }));

        // P14: inventory transitions that originate inside the daemon-owned
        // PairedHostService (not just renderer-driven pair/migrate/forget)
        // must reach the desktop. The sink feeds the daemon's remote-event
        // broadcast; the GUI's remote event bridge (lib.rs
        // start_remote_event_bridge) re-emits it under the same
        // paired_host_inventory_changed Tauri event name the renderer already
        // listens for.
        let mut paired_hosts = paired_hosts;
        {
            // Keep the gateway's CORS allowlist in step with the saved paired hosts. The lister
            // is cloned before the sink is installed, so it shares the inventory but not the sink.
            let cors_lister = paired_hosts.clone();
            let cors_state = Arc::clone(&remote_state);
            let inventory_sink_tx = remote_event_tx.clone();
            paired_hosts.set_event_sink(Arc::new(move |event| {
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    runtime.spawn(refresh_cors_saved_host_origins(
                        cors_lister.clone(),
                        Arc::clone(&cors_state),
                    ));
                }
                let payload = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
                let _ = inventory_sink_tx.send(DaemonRemoteEvent {
                    event: crate::ipc::paired_host::PAIRED_HOST_INVENTORY_CHANGED_EVENT.to_string(),
                    payload,
                });
            }));
            // Reachability changes ride the same broadcast; the GUI bridge forwards the raw
            // JSON under this event name, and an older GUI simply has no listener for it.
            let reachability_sink_tx = remote_event_tx.clone();
            paired_hosts.set_reachability_sink(Arc::new(move |entry| {
                let payload = serde_json::to_value(&entry).unwrap_or(serde_json::Value::Null);
                let _ = reachability_sink_tx.send(DaemonRemoteEvent {
                    event: crate::paired_host::reachability::PAIRED_HOST_REACHABILITY_CHANGED_EVENT.to_string(),
                    payload,
                });
            }));
        }

        let remote_handle_for_handover: Arc<
            Mutex<Option<crate::remote::server::RemoteServerHandle>>,
        > = Arc::new(Mutex::new(None));
        {
            let remote_state_cb = Arc::clone(&remote_state);
            let remote_handle_cb = Arc::clone(&remote_handle_for_handover);
            handover_manager.on_commit(move || {
                tracing::info!("Handover committed: tearing down legacy Remote Gateway listener and clearing active selection");
                let prev_handle = remote_handle_cb.lock().take();
                if let Some(handle) = prev_handle {
                    handle.stop();
                }
                remote_state_cb.clear_active_selection();
                *remote_state_cb.is_running.write() = false;
                *remote_state_cb.bound_address.write() = None;
            });
        }

        Self {
            paired_hosts,
            session_router,
            handover_manager,
            resource_sampler: Arc::new(crate::daemon::resource_usage::ResourceSampler::new()),
            terminal_service,
            workspace_registry,
            remote_state,
            remote_event_tx,
            remote_server_handle: remote_handle_for_handover,
            remote_configure_lock: tokio::sync::Mutex::new(()),
            epoch,
            binary_path,
            binary_mtime_ms,
            session_service,
            agent_state_endpoint: Mutex::new(None),
            agent_state_ingress_unavailable: std::sync::atomic::AtomicBool::new(false),
            agent_integration,
            #[cfg(not(unix))]
            transport_token: generate_transport_token(),
            published_endpoints: Mutex::new(Vec::new()),
            #[cfg(test)]
            helper_home: isolated_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            #[cfg(test)]
            _catalog_fixture: catalog_fixture,
            #[cfg(test)]
            close_park: std::sync::OnceLock::new(),
        }
    }

    /// Parks every later Close handler after its response flush (see close_park).
    #[cfg(test)]
    pub(crate) fn set_close_park_for_test(
        &self,
        park: Arc<(tokio::sync::Notify, tokio::sync::Notify)>,
    ) {
        assert!(self.close_park.set(park).is_ok(), "close park installed once");
    }

    /// Installs a loopback-permitting paired inventory for isolated fixtures.
    /// Production pairing always uses the HTTPS-only policy.
    #[cfg(test)]
    pub fn set_paired_hosts_for_test(
        &mut self,
        service: crate::paired_host::service::PairedHostService,
    ) {
        self.paired_hosts = service;
    }

    async fn handle_spawn(
        &self,
        client_request_id: &str,
        workspace_id: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        shell: Option<String>,
        startup: Option<TerminalStartup>,
    ) -> Result<String, SpawnError> {
        self.session_service
            .handle_spawn(
                client_request_id,
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                shell,
                startup,
                #[cfg(test)]
                self.helper_home.clone(),
            )
            .await
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn terminal_service(&self) -> &Arc<TerminalService> {
        &self.terminal_service
    }

    pub fn workspace_registry(&self) -> &WorkspaceRegistry {
        &self.workspace_registry
    }

    pub fn remote_state(&self) -> &Arc<RemoteGatewayState> {
        &self.remote_state
    }

    pub fn session_service(&self) -> &Arc<DaemonSessionService> {
        &self.session_service
    }

    pub(crate) async fn describe_session_identity(&self, session_id: &str) -> DaemonResponse {
        if self.session_router.is_local_session(session_id) {
            match self.handle_describe_session(session_id) {
                DaemonResponse::DescribeSessionOk { mut session, .. } => {
                    if let Some(pid) = self.terminal_service.get_session(session_id).and_then(|s| s.pid()) {
                        session.cwd = crate::ipc::run_blocking::<Option<PathBuf>, _>(move || {
                            Ok(crate::ipc::terminal::process_cwd(pid))
                        }).await.ok().flatten().map(|path| path.to_string_lossy().into_owned());
                    }
                    if let Some(ref mut auth) = session.recovery_authority {
                        auth.daemon_epoch = Some(self.epoch.to_string());
                    }
                    DaemonResponse::DescribeSessionOk {
                        session,
                        daemon_epoch: Some(self.epoch.to_string()),
                    }
                }
                other => other,
            }
        } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(session_id) {
            match peer.describe_session_identity(session_id).await {
                Ok((session, daemon_epoch)) => DaemonResponse::DescribeSessionOk {
                    session, daemon_epoch,
                },
                Err(message) => DaemonResponse::Error {
                    message,
                    code: Some("SESSION_NOT_FOUND".to_string()),
                    details: Some(serde_json::json!({
                        "source": "daemon_describe",
                        "kind": "owner_unavailable",
                        "sessionId": session_id,
                        "recoveryOutcome": "retryable",
                        "reason": "legacy_peer_unreachable",
                    })),
                },
            }
        } else {
            DaemonResponse::Error {
                message: format!("Session '{session_id}' not found"),
                code: Some("SESSION_NOT_FOUND".to_string()),
                details: Some(serde_json::json!({
                    "source": "daemon_describe",
                    "kind": "session_not_found",
                    "sessionId": session_id,
                    "recoveryOutcome": "retryable",
                    "reason": "scoped_missing",
                })),
            }
        }
    }

    /// The loopback TCP endpoint (`port`, bearer `token`) the agent-state ingress bound, or
    /// `None` while it has not bound. Unix exposes the socket path through
    /// [`agent_state_socket_path`] instead.
    pub fn agent_state_endpoint(&self) -> Option<(u16, String)> {
        self.agent_state_endpoint.lock().clone()
    }

    /// Bearer token every connection must present before a request is dispatched on
    /// loopback transports (Windows). Returns None on Unix where filesystem permissions
    /// enforce ownership.
    #[cfg(test)]
    pub(crate) fn transport_token(&self) -> Option<&str> {
        #[cfg(not(unix))]
        {
            Some(&self.transport_token)
        }
        #[cfg(unix)]
        {
            None
        }
    }

    /// True once this boot's agent-state ingress was observed to fail, so an absent endpoint is
    /// reported as a failure instead of being inferred from a `None` that also means "not yet".
    pub fn agent_state_ingress_unavailable(&self) -> bool {
        self.agent_state_ingress_unavailable
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Observes the agent-state ingress outcome, records it, and only then releases the readiness
    /// signal. That order is the contract: a client released by the signal can spawn a pane
    /// immediately, and the pane reads its agent-state endpoint as it spawns, so recording the
    /// outcome afterwards would announce a boot whose panes already lost the ingress.
    ///
    /// A missing ingress is recorded and warned about, never fatal. Terminals are the product and
    /// must keep working; agent-state reporting is only an observability ingress, so the
    /// degradation has to be visible and reportable rather than turned into a refused boot.
    fn settle_agent_state_ingress<S: FnOnce()>(
        &self,
        ingress: Option<tokio::task::JoinHandle<()>>,
        signal_ready: S,
    ) {
        // The handle is deliberately dropped rather than awaited: the ingress task, when there is
        // one, serves this boot for as long as it runs.
        if ingress.is_none() {
            self.agent_state_ingress_unavailable
                .store(true, std::sync::atomic::Ordering::Release);
            tracing::warn!(
                "Agent-state ingress unavailable for this boot: no agent-state endpoint was \
                 bound or published, so panes will not receive one and agent-state reporting is \
                 unavailable for their lifetime; terminals are unaffected"
            );
        }
        signal_ready();
    }

    /// Parses one newline-delimited extension report, rejecting states the UI cannot render.
    fn parse_agent_state_report(
        line: &str,
    ) -> Option<AgentStateIngressReport> {
        let report = serde_json::from_str::<AgentStateReport>(line.trim()).ok()?;
        Self::admit_agent_state_report(report)
    }

    /// Parses one herdr `pane.report_agent` line from a known herdr-reporting agent.
    #[cfg(any(unix, test))]
    fn parse_herdr_report(line: &str) -> Option<AgentStateIngressReport> {
        translate_herdr_report(line).and_then(Self::admit_agent_state_report)
    }

    /// Rejects states the UI cannot render and drops a provider session the agent cannot resume.
    fn admit_agent_state_report(report: AgentStateReport) -> Option<AgentStateIngressReport> {
        if !matches!(report.state.as_str(), "working" | "blocked" | "idle") {
            return None;
        }
        let provider_session = report.provider_session.filter(|provider| {
            report.agent.as_deref().is_some_and(|agent| {
                crate::terminal::shell::resolve_agent_resume_plan(agent, provider).is_ok()
            })
        });
        Some((
            report.session_id,
            report.state,
            report.agent,
            provider_session,
            report.detail,
        ))
    }

    /// The pid of a pane's own PTY, when it has a live one.
    fn ingress_pty_pid(&self, session_id: &str) -> Option<u32> {
        self.session_service
            .machine_pty(session_id)
            .and_then(|pty| pty.pid())
    }

    /// Is `claimed` the OMO store `provider_id` resolves to under this host's HOME?
    ///
    /// Both sides are canonicalized and the claim must be a regular file, so a symlink or a
    /// directory cannot pass by spelling alone.
    fn ingress_path_is_own_store(provider_id: &str, claimed: &str) -> bool {
        let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) else {
            return false;
        };
        let Some(expected) =
            crate::agent_transcript::transcript_path_for_session(Path::new(&home), provider_id)
        else {
            return false;
        };
        let (Ok(claimed_path), Ok(expected_path)) = (
            std::fs::canonicalize(Path::new(claimed)),
            std::fs::canonicalize(&expected),
        ) else {
            return false;
        };
        claimed_path.is_file() && claimed_path == expected_path
    }

    /// The identity admission must see for one report, or `None` when none may be admitted.
    ///
    /// Every process-table and filesystem read happens inside `run_blocking`: discovery walks the
    /// process table and reads a process environment, and the path check touches the filesystem, so
    /// neither may run on the reactor.
    async fn ingress_identity_for_admission(
        self: &Arc<Self>,
        session_id: &str,
        agent: Option<&str>,
        reported: Option<&AgentProviderSession>,
    ) -> Option<AgentProviderSession> {
        let Some(provider) = reported else {
            return None;
        };
        let Some(agent) = agent else {
            return Some(provider.clone());
        };
        if provider.transcript_path.is_none()
            || !crate::terminal::resume_cwd::machine_admits_agent(agent)
        {
            return Some(provider.clone());
        }
        let server = Arc::clone(self);
        let session_id = session_id.to_string();
        let agent = agent.to_string();
        let id = provider.id.clone();
        let claimed = provider.transcript_path.clone().unwrap_or_default();
        let decision = crate::ipc::run_blocking(move || {
            let discovered = server
                .ingress_pty_pid(&session_id)
                .and_then(|pid| crate::ipc::agents::discover_agent_session(pid, &agent))
                .and_then(|(found, source)| binding_discovery(&agent, found, source));
            // With no discovered id, an agent that keeps no process-bound session record is
            // checked against the id it reported; every other agent proves nothing without one.
            let store_id = discovered.clone().or_else(|| {
                agent_has_no_process_bound_session(&agent).then(|| id.trim().to_string())
            });
            let path_is_own_store = store_id.as_deref().is_some_and(|found| {
                if agent == "omo" {
                    // omo keeps its own resolver, which also searches the legacy flat store.
                    Self::ingress_path_is_own_store(found, &claimed)
                } else {
                    crate::ipc::file_link::home_dir().is_some_and(|home| {
                        ingress_path_is_agent_store(
                            &agent,
                            found,
                            Path::new(&claimed),
                            &crate::ipc::agent_history::provider_history_roots(&home),
                        )
                    })
                }
            });
            Ok(ingress_admission_identity(
                &agent,
                &id,
                true,
                discovered.as_deref(),
                path_is_own_store,
            ))
        })
        .await;
        match decision {
            Ok(IngressAdmissionIdentity::WithoutPath) => Some(AgentProviderSession {
                key: provider.key,
                id: provider.id.clone(),
                transcript_path: None,
            }),
            Ok(IngressAdmissionIdentity::AsReported) => Some(provider.clone()),
            Ok(IngressAdmissionIdentity::Refused) | Err(_) => None,
        }
    }

    /// Fold one parsed ingress report into the hub.
    ///
    /// A report is admitted as a machine session only when this daemon owns the session. A local
    /// pane keeps the metadata it reported, and a refused identity never replaces a published one.
    async fn ingest_agent_state_report(
        self: &Arc<Self>,
        states: &Arc<AgentStateHub>,
        sessions: &Arc<DaemonSessionService>,
        epoch: crate::scoped_contracts::Epoch,
        report: AgentStateIngressReport,
    ) {
        if let Some((_, state)) = self
            .resolve_agent_state_report(
                sessions,
                epoch,
                report,
                crate::daemon::protocol::AgentStateOrigin::Agent,
            )
            .await
        {
            states.publish_canonical(state);
        }
    }

    /// Runs one report through ingress identity, machine-owner validation and
    /// `admit_agent_report`, and returns the outcome with the state the hub may publish. `None`
    /// means nothing may be published (the session exited, expired, or its owner is unreachable).
    async fn resolve_agent_state_report(
        self: &Arc<Self>,
        sessions: &Arc<DaemonSessionService>,
        epoch: crate::scoped_contracts::Epoch,
        report: AgentStateIngressReport,
        origin: crate::daemon::protocol::AgentStateOrigin,
    ) -> Option<(IngressOutcome, AgentState)> {
        let (session_id, state, agent, reported, detail) = report;
        let admitted = self
            .ingress_identity_for_admission(&session_id, agent.as_deref(), reported.as_ref())
            .await;
        let outcome = match sessions.machine_detail_routed(&session_id, epoch).await {
            Ok(crate::remote::machine_protocol::SessionDetail::Running { session }) => {
                let hint = AgentStateReport {
                    session_id: session_id.clone(),
                    state: state.clone(),
                    agent: agent.clone(),
                    provider_session: admitted.clone(),
                    detail: detail.clone(),
                };
                match sessions.validate_machine_agent_report(session.target, hint).await {
                    Ok(_) => IngressOutcome::Admitted,
                    // Not machine-owned: this pane's local consumer owns its metadata.
                    Err(error) if error == "SESSION_NOT_FOUND" => IngressOutcome::Local,
                    Err(error) => {
                        tracing::debug!(%error, "Machine agent metadata rejected");
                        IngressOutcome::Refused
                    }
                }
            }
            // An exited or expired session, or an unreachable owner, publishes nothing.
            Ok(_) => return None,
            Err(error) if error == "SESSION_NOT_FOUND" => IngressOutcome::Local,
            Err(error) => {
                tracing::debug!(%error, "Machine agent owner unavailable");
                return None;
            }
        };
        Some((
            outcome,
            AgentState {
                session_id,
                state,
                agent,
                provider_session: ingress_publish_identity(outcome, reported, admitted),
                detail,
                origin,
            },
        ))
    }

    /// Re-explore-4 G2: gives a machine-owned pane the provider session its agent holds when the
    /// agent reports nothing itself (kimi, copilot and gjc have no reporter; any other
    /// `MACHINE_RESUME_AGENTS` member whose reporter is absent is covered the same way).
    ///
    /// `pty_pid` is the pane's own PTY process, the root `admit_agent_report` re-runs discovery
    /// from, so both reads agree on which process they mean. The synthesized report goes through
    /// the same `resolve_agent_state_report` path as an agent's own report, so machine-owner
    /// validation and `admit_agent_report` (discovered id, resume plan, store cwd) still decide
    /// it. `baseline` is the provider session retained when this agent process was first seen.
    ///
    /// Returns true when probing this agent process can stop: the id was admitted and published,
    /// or an identity is already present that discovery must not replace.
    async fn admit_discovered_machine_agent(
        self: &Arc<Self>,
        session_id: &str,
        agent: &'static str,
        pty_pid: u32,
        baseline: Option<AgentProviderSession>,
    ) -> bool {
        let discovered = crate::ipc::run_blocking(move || {
            Ok(crate::ipc::agents::discover_agent_session_id(pty_pid, agent))
        })
        .await
        .ok()
        .flatten();
        let Some(id) = discovered else {
            return false;
        };
        let retained = self.agent_states.current(session_id);
        let already_published = retained
            .as_ref()
            .and_then(|state| state.provider_session.as_ref())
            .is_some_and(|provider| provider.id == id);
        if already_published
            || crate::daemon::agent_state::holds_live_provider(retained.as_ref(), baseline.as_ref())
        {
            return true;
        }
        let report = discovered_machine_report(session_id, agent, &id, retained.as_ref());
        let sessions = self.session_service.clone();
        let resolved = self
            .resolve_agent_state_report(
                &sessions,
                crate::scoped_contracts::Epoch(self.epoch),
                report,
                crate::daemon::protocol::AgentStateOrigin::ProcessDiscovered,
            )
            .await;
        match resolved {
            // Only an admitted identity is published; a refusal or a local pane publishes nothing,
            // so a discovered guess can never reach the hub unverified.
            Some((IngressOutcome::Admitted, state)) if state.provider_session.is_some() => {
                self.agent_states.publish_discovered(state, baseline.as_ref());
                true
            }
            Some((outcome, _)) => {
                tracing::debug!(session_id, agent, ?outcome, "discovered machine identity not admitted");
                false
            }
            None => false,
        }
    }

    /// One observer tick's G2 bookkeeping for a pane whose foreground is `agent` (process
    /// `agent_pid`): starts a probe when one is due, rate-limited by
    /// `machine_identity_probe_delay` and bounded by `MACHINE_IDENTITY_PROBE_ATTEMPTS`. A new
    /// agent process in the pane re-arms the budget. The probe itself, including the
    /// machine-ownership read, runs off the tick and reports back through `done`.
    fn schedule_machine_identity_probe(
        self: &Arc<Self>,
        probes: &mut HashMap<String, MachineIdentityProbe>,
        done: &tokio::sync::mpsc::UnboundedSender<(String, u32, bool)>,
        session_id: &str,
        agent_pid: u32,
        agent: &'static str,
    ) {
        if !crate::terminal::resume_cwd::machine_admits_agent(agent) {
            probes.remove(session_id);
            return;
        }
        let now = tokio::time::Instant::now();
        let same_process = probes
            .get(session_id)
            .is_some_and(|probe| probe.agent_pid == agent_pid && probe.agent == agent);
        if !same_process {
            let baseline = self
                .agent_states
                .current(session_id)
                .and_then(|state| state.provider_session);
            probes.insert(
                session_id.to_string(),
                MachineIdentityProbe {
                    agent_pid,
                    agent,
                    baseline,
                    attempts: 0,
                    next_at: now,
                    in_flight: false,
                    finished: false,
                },
            );
        }
        let Some(probe) = probes.get_mut(session_id) else {
            return;
        };
        if probe.finished || probe.in_flight || now < probe.next_at {
            return;
        }
        if crate::daemon::agent_state::holds_live_provider(
            self.agent_states.current(session_id).as_ref(),
            probe.baseline.as_ref(),
        ) {
            probe.finished = true;
            return;
        }
        let Some(delay) = machine_identity_probe_delay(probe.attempts) else {
            probe.finished = true;
            return;
        };
        let Some(pty_pid) = self
            .terminal_service
            .get_session(session_id)
            .and_then(|session| session.pid())
        else {
            probe.finished = true;
            return;
        };
        probe.attempts += 1;
        probe.next_at = now + delay;
        probe.in_flight = true;
        let baseline = probe.baseline.clone();
        let server = Arc::clone(self);
        let done = done.clone();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            let machine_only = server
                .session_service
                .machine_only_async(&session_id)
                .await
                .unwrap_or(false);
            let holds = crate::daemon::agent_state::holds_live_provider(
                server.agent_states.current(&session_id).as_ref(),
                baseline.as_ref(),
            );
            let finished = if machine_identity_probe_wanted(agent, machine_only, holds) {
                server
                    .admit_discovered_machine_agent(&session_id, agent, pty_pid, baseline)
                    .await
            } else {
                // A local pane's UI probes for itself, and an identity already present stays.
                true
            };
            let _ = done.send((session_id, agent_pid, finished));
        });
    }

    #[cfg(unix)]
    pub fn spawn_agent_state_listener(self: &Arc<Self>) -> Option<tokio::task::JoinHandle<()>> {
        use crate::daemon::handover::AuthorityListenerRole;
        let path = get_agent_state_socket_path();
        // A committed HandoverCandidate keeps serving the agent-state listener it inherited
        // (todo 8): the path panes already hold never disappears.
        let inherited = self
            .handover_manager
            .take_inherited_listener(AuthorityListenerRole::AgentState)
            .and_then(|listener| match UnixListener::from_std(listener) {
                Ok(listener) => Some(listener),
                Err(error) => {
                    tracing::warn!(%error, "Failed to serve the inherited agent state listener");
                    None
                }
            });
        let listener = match inherited {
            Some(listener) => listener,
            None => {
                let _ = fs::remove_file(&path);
                let listener = match UnixListener::bind(&path) {
                    Ok(listener) => listener,
                    Err(error) => {
                        tracing::warn!(%error, "Failed to bind agent state socket");
                        return None;
                    }
                };
                if let Err(error) = fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
                    tracing::warn!(%error, "Failed to secure agent state socket");
                }
                listener
            }
        };
        {
            use std::os::fd::AsRawFd;
            if let Err(error) = self
                .handover_manager
                .register_authority_listener(AuthorityListenerRole::AgentState, listener.as_raw_fd())
            {
                tracing::warn!(%error, "Agent state listener cannot be handed to a successor");
            }
        }

        let server = Arc::clone(self);
        let states = Arc::clone(&self.agent_states);
        let sessions = self.session_service.clone();
        let epoch = crate::scoped_contracts::Epoch(self.epoch);
        let manager = Arc::clone(&self.handover_manager);
        Some(tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                // None: the listener belongs to the successor now; close this copy.
                let Some(accepted) = manager.gated_accept(|| listener.accept()).await else {
                    drop(listener);
                    break;
                };
                let Ok((stream, _)) = accepted else {
                    continue;
                };
                manager.note_accept(AuthorityListenerRole::AgentState);
                let states = Arc::clone(&states);
                let sessions = sessions.clone();
                let server = Arc::clone(&server);
                while clients.try_join_next().is_some() {}
                clients.spawn(server.serve_agent_state_connection(states, sessions, epoch, stream));
            }
        }))
    }

    /// Serves one connection on the unix agent-state socket: Ferryx's own `agentState` reports,
    /// and herdr `pane.*` requests from agents that report through `HERDR_SOCKET_PATH`.
    ///
    /// herdr lines carry no token. They are accepted only here, where the 0600 socket is the
    /// ownership boundary, and never on the loopback TCP ingress, which needs this boot's token.
    #[cfg(unix)]
    async fn serve_agent_state_connection(
        self: Arc<Self>,
        states: Arc<AgentStateHub>,
        sessions: Arc<DaemonSessionService>,
        epoch: crate::scoped_contracts::Epoch,
        stream: tokio::net::UnixStream,
    ) {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            if let Some(response) = herdr_response_line(&line) {
                // Answer before ingesting: the herdr client gives up after 500 ms, and admission
                // may wait on a blocking journal read.
                if let Err(error) = write_half.write_all(response.as_bytes()).await {
                    tracing::debug!(%error, "herdr client left before its reply");
                }
                if let Some(report) = Self::parse_herdr_report(&line) {
                    self.ingest_agent_state_report(&states, &sessions, epoch, report)
                        .await;
                }
            } else if let Some(report) = Self::parse_agent_state_report(&line) {
                self.ingest_agent_state_report(&states, &sessions, epoch, report)
                    .await;
            }
            line.clear();
        }
    }

    /// Windows/Linux counterpart of the unix socket ingress: a loopback TCP listener the bundled
    /// extension reaches through `FERRYX_AGENT_STATE_PORT`/`FERRYX_AGENT_STATE_TOKEN`, the same
    /// TCP mode remote sessions already use. Any local process can connect to a loopback port,
    /// so a report is accepted only when it carries this boot's token.
    #[cfg(not(unix))]
    pub fn spawn_agent_state_listener(self: &Arc<Self>) -> Option<tokio::task::JoinHandle<()>> {
        // The previous boot's record is dropped before this boot publishes: a listener that fails
        // to bind below must not leave the old boot's endpoint looking valid. The reported
        // endpoint is dropped with it, so no failure path below can leave this boot reporting an
        // endpoint while nothing is discoverable on disk.
        clear_stale_agent_state_rendezvous(&get_runtime_dir());
        *self.agent_state_endpoint.lock() = None;
        let bound = match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)) {
            Ok(bound) => bound,
            Err(error) => {
                tracing::warn!(%error, "Failed to bind agent state listener");
                return None;
            }
        };
        if let Err(error) = bound.set_nonblocking(true) {
            tracing::warn!(%error, "Failed to configure agent state listener");
            return None;
        }
        let port = match bound.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                tracing::warn!(%error, "Failed to read agent state listener address");
                return None;
            }
        };
        let listener = match TcpListener::from_std(bound) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "Failed to register agent state listener");
                return None;
            }
        };
        let token = generate_transport_token();
        // A pane reads the endpoint out of this record as it spawns, so it must be on disk before
        // the accept loop admits a connection. Port and token travel as one renamed file, so a
        // reader can never observe one half of a pair, and the endpoint is recorded only once that
        // file exists.
        if let Err(error) = publish_agent_state_endpoint(
            &self.agent_state_endpoint,
            &get_runtime_dir(),
            port,
            &token,
        ) {
            tracing::warn!(%error, "Failed to publish agent state rendezvous");
            return None;
        }
        tracing::info!(port, "Agent state ingress listening on loopback");

        let server = Arc::clone(self);
        let states = Arc::clone(&self.agent_states);
        let sessions = self.session_service.clone();
        let epoch = crate::scoped_contracts::Epoch(self.epoch);
        Some(tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let states = Arc::clone(&states);
                let sessions = sessions.clone();
                let token = token.clone();
                let server = Arc::clone(&server);
                while clients.try_join_next().is_some() {}
                clients.spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                        let presented = transport_token_from_line(&line);
                        if !transport_token_matches(&token, presented.as_deref()) {
                            tracing::debug!(
                                "Agent state report rejected: token missing or invalid"
                            );
                            line.clear();
                            continue;
                        }
                        if let Some(report) = Self::parse_agent_state_report(&line) {
                            server
                                .ingest_agent_state_report(&states, &sessions, epoch, report)
                                .await;
                        }
                        line.clear();
                    }
                });
            }
        }))
    }

    /// Probes every machine-granted paired host at start and then every `interval`
    /// (production passes PROBE_INTERVAL). It holds only a weak server reference and stops
    /// with the daemon, or as soon as a committed handover leaves this daemon Draining or
    /// Retired: the successor owns paired-hosts.v1.json from then on, and a probe here could
    /// revoke a host and rewrite that file under the successor (which then fences itself).
    fn spawn_paired_host_probe(self: &Arc<Self>, interval: Duration) -> tokio::task::JoinHandle<()> {
        let server = Arc::downgrade(self);
        let service = self.paired_hosts.clone();
        tokio::spawn(crate::paired_host::client::run_probe_loop(
            service,
            interval,
            move || Self::owns_paired_host_inventory(&server),
        ))
    }

    /// True while the server is alive and has not handed its authority to a successor.
    fn owns_paired_host_inventory(server: &std::sync::Weak<Self>) -> bool {
        server.upgrade().is_some_and(|server| {
            !matches!(
                server.handover_manager.status(),
                super::handover::HandoverStatus::Draining | super::handover::HandoverStatus::Retired
            )
        })
    }

    /// Forwards direct/relay route transitions to the GUI under `paired_host_route_changed`.
    /// The subscription is taken here, synchronously, before the accept loop can serve a
    /// `PairedHostRouteList`, so a transition never falls between a list and its events.
    fn spawn_paired_host_route_forwarder(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let routes = crate::paired_host::direct_route::GLOBAL_DIRECT_ROUTES.clone();
        let events = routes.subscribe();
        let server = Arc::downgrade(self);
        let service = self.paired_hosts.clone();
        tokio::spawn(crate::paired_host::route_view::forward_route_events(
            routes,
            events,
            self.remote_event_tx.clone(),
            move || {
                let service = service.clone();
                async move {
                    service
                        .list()
                        .await
                        .map(|hosts| hosts.into_iter().map(|host| host.host_id).collect::<Vec<_>>())
                }
            },
            move || server.strong_count() > 0,
        ))
    }

    fn spawn_foreground_observer(self: &Arc<Self>) {
        let server = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut transitions =
                HashMap::<String, crate::terminal::foreground::ProcessTransition>::new();
            // Re-explore-4 G2: per-pane discovery of the provider session an agent that reports
            // nothing holds. Probes run off the tick and post back whether to stop.
            let mut probes = HashMap::<String, MachineIdentityProbe>::new();
            let (probe_done_tx, mut probe_done) =
                tokio::sync::mpsc::unbounded_channel::<(String, u32, bool)>();
            // Inspect even without output. This cadence only collects positive process
            // evidence; neither elapsed time nor silence can release activity.
            let mut sample = tokio::time::interval(std::time::Duration::from_millis(250));
            sample.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                sample.tick().await;
                let Some(server) = server.upgrade() else {
                    break;
                };
                let sessions: Vec<_> = server
                    .terminal_service
                    .list_sessions()
                    .into_iter()
                    .filter_map(|id| {
                        server
                            .terminal_service
                            .get_session(&id)
                            .map(|session| (id, session))
                    })
                    .collect();
                transitions.retain(|id, _| sessions.iter().any(|(live, _)| live == id));
                probes.retain(|id, _| sessions.iter().any(|(live, _)| live == id));
                while let Ok((id, agent_pid, finished)) = probe_done.try_recv() {
                    if let Some(probe) = probes.get_mut(&id).filter(|p| p.agent_pid == agent_pid) {
                        probe.in_flight = false;
                        probe.finished |= finished;
                    }
                }
                // No live session means nothing to classify: skip the process listing.
                if sessions.is_empty() {
                    continue;
                }
                let observations = crate::ipc::run_blocking(move || {
                    // One process listing per tick. On Windows that listing is a full
                    // PowerShell enumeration, so capturing it per session multiplied the
                    // cost by the pane count; every session in this tick is classified
                    // against the same sample. A failed capture is unknown ownership for
                    // all of them, which is the same evidence a failed per-session
                    // inspection already produced.
                    let snapshot = crate::terminal::foreground::capture_process_snapshot();
                    Ok(sessions
                        .into_iter()
                        .map(|(id, session)| {
                            let observation = match &snapshot {
                                Ok(snapshot) => {
                                    crate::terminal::foreground::inspect_with_snapshot(
                                        &session, snapshot,
                                    )
                                }
                                Err(error) => Err(std::io::Error::new(
                                    error.kind(),
                                    error.to_string(),
                                )),
                            };
                            (id, observation)
                        })
                        .collect::<Vec<_>>())
                })
                .await;
                match observations {
                    Ok(observations) => {
                        for (id, observation) in observations {
                            match observation {
                                Ok(observation) => {
                                    let edge = transitions
                                        .entry(id.clone())
                                        .or_default()
                                        .observe(observation);
                                    match observation {
                                        Some(crate::terminal::foreground::Foreground::Agent {
                                            pid,
                                            agent,
                                        }) => {
                                            server.schedule_machine_identity_probe(
                                                &mut probes,
                                                &probe_done_tx,
                                                &id,
                                                pid,
                                                agent,
                                            );
                                        }
                                        // The agent left the foreground: stop probing for it.
                                        Some(_) => {
                                            probes.remove(&id);
                                        }
                                        None => {}
                                    }
                                    if server.terminal_service.get_session(&id).is_some() {
                                        match edge {
                                        Some(crate::terminal::foreground::AgentProcessEdge::Released) => {
                                            server.agent_states.release_foreground(&id);
                                            server.session_service.release_provider_claim(&id);
                                        }
                                        Some(crate::terminal::foreground::AgentProcessEdge::Observed) => {
                                            server.agent_states.observe_foreground_agent(&id);
                                        }
                                        None => {}
                                    }
                                    }
                                }
                                Err(error) => {
                                    tracing::debug!(session_id = id, %error, "foreground inspection unavailable; holding state")
                                }
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "foreground observer failed; holding state")
                    }
                }
            }
        });
    }

    /// Records why this daemon is going away, then flushes the state that outlives it.
    ///
    /// Local PTYs die with the process, so the only durable state worth writing is the remote
    /// session table: it is what lets a successor re-attach those sessions to the same ids. The
    /// reason is written first because a daemon death with no record at all is the state the
    /// 2026-10-07 incident had to be reconstructed from, and it could not be.
    pub async fn shutdown_gracefully(&self, reason: &str) {
        tracing::warn!(reason, "Daemon shutting down");
        if let Err(message) = self
            .persist_remote_sessions_at(self.remote_sessions_path.clone())
            .await
        {
            tracing::error!(%message, "Failed to persist remote sessions while shutting down");
        }
        // A clean exit takes its public endpoint with it, so the next launcher sees Missing.
        // Only an owner (it still holds the daemon locks) published the files on disk; a
        // draining predecessor handed its locks and listeners to the successor and leaves them.
        if self.handover_manager.holds_lock_files() {
            let artifacts = self.published_endpoints.lock().clone();
            let removed =
                crate::ipc::run_blocking(move || Ok(remove_owned_endpoint_artifacts(&artifacts))).await;
            tracing::info!(?removed, "Removed this owner's public endpoint artifacts");
        }
    }

    /// Records a public endpoint file this owner serves (data/control socket, or on loopback
    /// transports the port and token files), with the identity it has right now.
    pub(crate) fn note_published_endpoint(&self, path: &Path) {
        if let Some(identity) = endpoint_artifact_identity(path) {
            self.published_endpoints.lock().push(EndpointArtifact {
                path: path.to_path_buf(),
                identity,
            });
        }
    }

    pub async fn run_server(self: Arc<Self>) -> Result<(), String> {
        self.run_server_with_handover_and_readiness(None, None)
            .await
    }

    pub async fn run_server_with_handover(
        self: Arc<Self>,
        handover_from: Option<PathBuf>,
    ) -> Result<(), String> {
        self.run_server_with_handover_and_readiness(handover_from, None)
            .await
    }

    pub async fn run_server_with_handover_and_readiness(
        self: Arc<Self>,
        #[cfg_attr(not(unix), allow(unused_mut))] mut handover_from: Option<PathBuf>,
        ready_tx: Option<tokio::sync::oneshot::Sender<()>>,
    ) -> Result<(), String> {
        let runtime_dir = get_runtime_dir();
        ensure_runtime_directory(&runtime_dir)?;

        // HandoverCandidate admission (todo 8). A successor generation's OS job runs plain
        // `--daemon`; before any lock is taken it reads the predecessor's handover-request.json.
        // A valid request for this generation from a live predecessor makes this start the
        // existing --handover-from path; an invalid, expired or foreign request is deleted and
        // this process exits 0 holding no lock; no request means an ordinary start.
        #[cfg(unix)]
        let mut handover_expires_at: Option<u64> = None;
        #[cfg(unix)]
        if handover_from.is_none() {
            let dir = runtime_dir.clone();
            // A daemon from before the rename writes its request for this generation into its
            // own runtime dir; only a request for this generation is taken from there.
            let legacy_dir = get_legacy_runtime_dir().filter(|legacy| *legacy != dir);
            let role = crate::ipc::run_blocking(move || {
                use crate::daemon::handover_request::{
                    own_generation_key, request_path, take_startup_request, HandoverRequest,
                    StartupRole,
                };
                let own_key = own_generation_key();
                let take = |dir: &Path| {
                    take_startup_request(
                        dir,
                        own_key.as_deref(),
                        crate::daemon::owner_admission::unix_now_ms(),
                        &crate::daemon::owner_admission::process_start_time,
                    )
                };
                let role = take(dir.as_path());
                if role != StartupRole::Ordinary {
                    return Ok(role);
                }
                let Some(legacy_dir) = legacy_dir else {
                    return Ok(role);
                };
                let for_this_generation = matches!(
                    crate::daemon::owner_admission::read_record::<HandoverRequest>(&request_path(&legacy_dir)),
                    Ok(Some(request)) if own_key.as_deref() == Some(request.generation_key.as_str())
                );
                Ok(if for_this_generation { take(legacy_dir.as_path()) } else { role })
            })
            .await
            .map_err(|error| error.to_string())?;
            match role {
                crate::daemon::handover_request::StartupRole::Ordinary => {}
                crate::daemon::handover_request::StartupRole::Candidate {
                    legacy_socket_path,
                    expires_at_unix_ms,
                } => {
                    tracing::info!(
                        legacy_socket = %legacy_socket_path.display(),
                        "Starting as HandoverCandidate from the predecessor's handover request"
                    );
                    handover_from = Some(legacy_socket_path);
                    handover_expires_at = Some(expires_at_unix_ms);
                }
                crate::daemon::handover_request::StartupRole::Exit { reason } => {
                    tracing::warn!(%reason, "Handover request rejected; exiting 0 without taking any lock");
                    // A Type=notify unit that exits before READY=1 counts as failed; report
                    // ready so this clean exit is not restarted.
                    crate::daemon::supervisor_linux::notify_ready();
                    return Ok(());
                }
            }
        }

        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        crate::daemon::qa_producers::install_and_start(&self).await;
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        self.handover_manager
            .inject_commit_fault(crate::daemon::qa_producers::commit_fault_from_env());

        let socket_path = get_socket_path();
        let lock_path = get_lock_path();
        // A committed HandoverCandidate serves the predecessor's own public listener and holds
        // its locks through the descriptors it inherited (todo 8): no bind and no lock
        // acquisition happen after commit, so the locks are never free and the public socket
        // path never disappears.
        #[cfg(unix)]
        let mut inherited_canonical: Option<std::os::unix::net::UnixListener> = None;
        // Set when the predecessor ran before the rename: its public socket is the legacy one,
        // which this daemon keeps serving for one release, and its locks are already installed.
        #[cfg(unix)]
        let mut inherited_legacy_compat: Option<std::os::unix::net::UnixListener> = None;
        #[cfg(unix)]
        let mut cross_rename_locks_installed = false;
        #[cfg(unix)]
        let cross_rename = handover_from.as_deref().and_then(Path::parent).is_some_and(|parent| {
            parent != runtime_dir.as_path()
                && get_legacy_runtime_dir().as_deref() == Some(parent)
        });

        if let Some(ref legacy_path) = handover_from {
            tracing::info!(
                "Starting daemon with handover from {}",
                legacy_path.display()
            );
            // Strictly validate that legacy socket is in the expected runtime directory,
            // owned by current user, mode 0700 parent directory, and not a symlink.
            validate_runtime_socket_path(legacy_path)?;
            // Only unix can be handed sessions: `spawn_legacy_handover_daemon` and the
            // `--handover-from` producer are unix-only, so the peer has no other consumer.
            #[cfg(unix)]
            let legacy_peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
                legacy_path.clone(),
                Vec::new(),
            ));

            #[cfg(unix)]
            if crate::daemon::handover::is_v5_ownership_transfer_enabled() {
                let listed = match legacy_peer.list_sessions().await {
                    Ok(sessions) => sessions,
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            "Failed to list predecessor sessions before handover transfer"
                        );
                        Vec::new()
                    }
                };

                let transfer_id = uuid::Uuid::new_v4().to_string();
                let mut expected_owners = HashMap::new();
                for session_id in &listed {
                    let response = legacy_peer.send_request(&DaemonRequest::DescribeSession {
                        session_id: session_id.clone(),
                    }).await?;
                    if let DaemonResponse::DescribeSessionOk { session, .. } = response {
                        expected_owners.insert(session_id.clone(), session);
                    }
                }
                let mut adopted_sessions = Vec::new();
                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                let mut adopted_records: Vec<crate::daemon::qa_producers::HandoverSessionRecord> =
                    Vec::new();
                let mut commit_started = false;
                // Set when the request's expiry passed before CommitHandover: the candidate then
                // aborts and exits 0, and the predecessor keeps serving (todo 8).
                let mut expired_before_commit = false;
                // The predecessor's authority descriptors, validated but not yet used. Dropping
                // them only closes this process's copies: the predecessor keeps serving.
                let mut received_authority: Option<crate::daemon::handover_socket::TransferredAuthority> = None;
                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                crate::daemon::qa_producers::note_authority_stage("candidate-admitted", &self.handover_manager).await;
                let handover_socket_path =
                    crate::daemon::handover_socket::get_handover_socket_path(&transfer_id);
                let listener = crate::daemon::handover_socket::HandoverSocketListener::bind(
                    &handover_socket_path,
                )
                .map_err(|e| format!("Failed to bind handover socket: {e}"))?;

                let accept_handle = tokio::task::spawn_blocking(move || {
                    let (stream, _creds) = listener.accept().map_err(|error| (error, 0usize))?;
                    let mut exports = Vec::new();
                    // Stopping at the first error silently strands every session behind it: the
                    // predecessor has already given up ownership of them, so an early break here
                    // is what turns a transport hiccup into dead terminals. Surface the error
                    // instead, and let the caller decide whether the handover may proceed.
                    loop {
                        match crate::daemon::handover_socket::recv_session_with_owner(&stream) {
                            Ok(Some(frame)) => exports.push(frame),
                            Ok(None) => break,
                            Err(error) => return Err((error, exports.len())),
                        }
                    }
                    Ok::<_, (crate::daemon::handover_socket::HandoverSocketError, usize)>((stream, exports))
                });

                let transfer_outcome: Result<(usize, usize, usize, u64), String> = async {
                    let transfer_sessions = DaemonRequest::TransferSessions {
                        handover_socket_path: handover_socket_path
                            .to_string_lossy()
                            .into_owned(),
                    };
                    let transfer_request = legacy_peer.send_request(&transfer_sessions);
                    // The request's expiry bounds the predecessor's answer as well (todo 8).
                    let transfer_resp = match handover_expires_at {
                        Some(expires) => {
                            let budget = crate::daemon::handover_request::pre_commit_remaining(
                                expires,
                                crate::daemon::owner_admission::unix_now_ms(),
                            )
                            .inspect_err(|_| expired_before_commit = true)?;
                            match tokio::time::timeout(budget, transfer_request).await {
                                Ok(answer) => answer,
                                Err(_) => {
                                    expired_before_commit = true;
                                    return Err(expiry_abort_reason(
                                        &handover_socket_path,
                                        "handover request expired during TransferSessions",
                                    )
                                    .await);
                                }
                            }
                        }
                        None => transfer_request.await,
                    }
                    .map_err(|e| format!("TransferSessions request failed: {e}"))?;

                    let (offered, owned) = match transfer_resp {
                        DaemonResponse::TransferSessionsOk {
                            transferred_count,
                            requested_count,
                        } => (transferred_count, requested_count),
                        other => return Err(format!("TransferSessions failed: {other:?}")),
                    };

                    // A transfer that stalls past the request's expiry is abandoned before commit.
                    let transfer_budget = match handover_expires_at {
                        Some(expires) => crate::daemon::handover_request::pre_commit_remaining(
                            expires,
                            crate::daemon::owner_admission::unix_now_ms(),
                        )
                        .inspect_err(|_| expired_before_commit = true)?,
                        None => std::time::Duration::MAX,
                    };
                    // QA only: todo 8's request-expiry variant, armed by the runner for this
                    // handover. The real mid-transfer expiry path below runs, deterministically.
                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                    if handover_expires_at.is_some() && crate::daemon::qa_producers::request_expiry_requested() {
                        expired_before_commit = true;
                        return Err(expiry_abort_reason(
                            &handover_socket_path,
                            "handover request expired mid-transfer (QA request-expired-mid-transfer)",
                        )
                        .await);
                    }
                    let received = match tokio::time::timeout(transfer_budget, accept_handle).await {
                        Ok(received) => received,
                        Err(_) => {
                            expired_before_commit = true;
                            // Wake the blocking accept so its worker returns instead of
                            // keeping this process alive after the clean exit.
                            return Err(expiry_abort_reason(
                                &handover_socket_path,
                                "handover request expired mid-transfer",
                            )
                            .await);
                        }
                    };
                    let (handover_stream, exports) = received
                        .map_err(|e| format!("Handover worker panicked: {e}"))?
                        .map_err(|(error, received)| {
                            format!(
                                "Handover socket receive error after {received} of {offered} session(s): {error}"
                            )
                        })?;
                    let accepted = exports.len();
                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                    crate::daemon::qa_producers::note_authority_stage("sessions-received", &self.handover_manager).await;
                    if let Some(expires) = handover_expires_at {
                        crate::daemon::handover_request::pre_commit_remaining(
                            expires,
                            crate::daemon::owner_admission::unix_now_ms(),
                        )
                        .inspect_err(|_| expired_before_commit = true)?;
                    }
                    let exported: std::collections::HashSet<String> =
                        exports.iter().map(|(export, _owner)| export.session_id.clone()).collect();
                    let durable_path = self.remote_sessions_path.clone();
                    let durable = crate::ipc::run_blocking(move || {
                        if !durable_path.exists() {
                            return Ok(std::collections::HashSet::new());
                        }
                        let raw = match std::fs::read_to_string(&durable_path) {
                            Ok(r) => r,
                            Err(error) => {
                                tracing::warn!(
                                    %error,
                                    path = %durable_path.display(),
                                    "Failed to read durable remote sessions snapshot for \
                                     handover accounting"
                                );
                                return Ok(std::collections::HashSet::new());
                            }
                        };
                        let value: serde_json::Value = match serde_json::from_str(&raw) {
                            Ok(v) => v,
                            Err(error) => {
                                tracing::warn!(
                                    %error,
                                    path = %durable_path.display(),
                                    "Failed to parse durable remote sessions snapshot for \
                                     handover accounting"
                                );
                                return Ok(std::collections::HashSet::new());
                            }
                        };
                        let set = value["remoteSessions"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|record| {
                                        record["descriptor"]["backendSessionId"]
                                            .as_str()
                                            .map(ToString::to_string)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        Ok::<_, crate::ipc::error::IpcError>(set)
                    })
                    .await
                    .unwrap_or_else(|error| {
                        tracing::warn!(
                            %error,
                            "Durable remote snapshot read failed during handover accounting"
                        );
                        std::collections::HashSet::new()
                    });
                    let restorable =
                        handover_restorable_unexported(&listed, &exported, &durable);
                    let listed_unique = listed
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len();
                    let effective_owned =
                        handover_effective_owned(owned, listed_unique, restorable);
                    // A predecessor that reports more than it delivered is the shape of a lossy
                    // handover: the sessions missing from `accepted` are the ones that lose their
                    // PTY owner when the predecessor retires. `owned` is what the predecessor
                    // actually held; `offered` already excludes sessions it failed to export, so
                    // checking `accepted` against `offered` alone cannot see those casualties.
                    tracing::info!(
                        owned,
                        listed_unique,
                        offered,
                        accepted,
                        restorable,
                        effective_owned,
                        "Received session exports over the handover socket"
                    );
                    if let Err(reason) =
                        handover_delivery_verdict_owned(effective_owned, offered, accepted)
                    {
                        // Do NOT proceed. Committing here retires the predecessor, and every
                        // session it offered but did not deliver dies with the PTY master fd it
                        // still owned. Aborting leaves the predecessor serving all of them instead.
                        return Err(reason);
                    }

                    for (export, owner) in exports {
                        tracing::info!(session_id = %export.session_id, "Adopting transferred session from predecessor");
                        let (master, snapshot) = export.into_parts();
                        let session_id = snapshot.session_id.clone();
                        let expected = expected_owners.get(&session_id).ok_or_else(||
                            format!("Predecessor cannot prove identity domain of '{session_id}'"))?;
                        if expected.incarnation.is_none() || expected.incarnation != snapshot.incarnation {
                            return Err(format!("Predecessor incarnation does not match export '{session_id}'; source retained"));
                        }
                        // This daemon has never spawned the session, so it holds no spawn record of
                        // its own: ownership transfers only through the record the predecessor
                        // exported, checked against the predecessor's own describe answer. Until
                        // that record is installed here this daemon is not an authoritative owner
                        // and must not take the PTY master.
                        adopt_transferred_ownership(&self.session_metadata, &session_id, expected, owner)?;
                        self.terminal_service.pty_manager().expect_transferred_owner(snapshot.clone())
                            .map_err(|error| {
                                release_adopted_ownership(&self.session_metadata, std::slice::from_ref(&session_id));
                                error.to_string()
                            })?;
                        let output_rx = match self
                            .terminal_service
                            .pty_manager()
                            .adopt_transferred_session(master, snapshot)
                        {
                            Ok(output_rx) => output_rx,
                            Err(error) => {
                                // The session stays with the predecessor, so this daemon must not
                                // keep claiming its workspace, nor keep the predecessor identity it
                                // installed just above: only a successful adopt removes that entry,
                                // so leaving it would hold the moved snapshot for the daemon's
                                // lifetime.
                                release_adopted_ownership(&self.session_metadata, std::slice::from_ref(&session_id));
                                self.terminal_service.pty_manager().forget_transferred_owner(&session_id);
                                return Err(format!("Failed to adopt transferred session: {error}"));
                            }
                        };
                        adopted_sessions.push(session_id.clone());
                        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                        {
                            let mut record =
                                crate::daemon::qa_producers::HandoverSessionRecord::new(
                                    session_id.clone(),
                                    expected.incarnation.clone(),
                                );
                            record.adopted_incarnation =
                                crate::daemon::qa_producers::describe_incarnation(self.as_ref(), &session_id);
                            record.adopted_readers_installed = 1;
                            record.adopted_reader_live =
                                crate::daemon::qa_producers::adopted_reader_live(
                                    &self.terminal_service,
                                    &session_id,
                                );
                            adopted_records.push(record);
                            if let Some(channel) = crate::daemon::qa_producers::channel().await {
                                crate::daemon::qa_producers::note_successor_adopt_off_runtime(
                                    &channel,
                                    &session_id,
                                )
                                .await;
                            }
                        }
                        // The receiver must be held and pumped for as long as the adopted child runs.
                        // Dropping it here marks the session's output channel closed, and the lifecycle
                        // watcher would then close the session -- terminating the very child this
                        // handover exists to preserve.
                        self.terminal_service.pump_adopted_output(session_id, output_rx);
                    }

                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                    {
                        if crate::daemon::qa_producers::abort_after_adopt_requested() {
                            // The runner's partial-adopt abort variant: the sessions really
                            // arrived and were really adopted, so the rollback below is the
                            // production relinquish-then-resume path, not a simulated one.
                            return Err("QA-injected abort after adopt".to_string());
                        }
                    }

                    // Authority transfer (todo 8): over the same handover socket, before
                    // CommitHandover, the candidate receives duplicates of the predecessor's
                    // public listeners and lock fds and validates every one of them. It accepts on
                    // none. Any failure here aborts before commit and the predecessor keeps
                    // serving. A pre-plan predecessor closes the stream without answering; only
                    // then does this candidate fall back to taking the locks after commit.
                    // A predecessor from before the rename serves every endpoint from the legacy
                    // runtime dir, so its authority is validated against those paths.
                    let authority_dir = if cross_rename {
                        legacy_path.parent().map(Path::to_path_buf).unwrap_or_else(|| runtime_dir.clone())
                    } else {
                        runtime_dir.clone()
                    };
                    let expected = crate::daemon::handover_socket::ExpectedAuthority {
                        canonical_socket: if cross_rename { authority_dir.join("daemon.sock") } else { socket_path.clone() },
                        legacy_lock: if cross_rename { authority_dir.join("daemon.lock") } else { lock_path.clone() },
                        persistent_lock: get_persistent_lock_path(),
                        control_socket: (!control_server::control_disabled_for_qa()).then(|| {
                            if cross_rename {
                                authority_dir.join(crate::daemon::control::CONTROL_SOCKET_FILE)
                            } else {
                                control_server::control_socket_path()
                            }
                        }),
                        agent_state_socket: Some(if cross_rename {
                            authority_dir.join("agent-state.sock")
                        } else {
                            get_agent_state_socket_path()
                        }),
                    };
                    let authority_transfer_id = transfer_id.clone();
                    received_authority = tokio::task::spawn_blocking(move || {
                        use crate::daemon::handover_socket::{
                            predecessor_closed_without_authority, request_authority, validate_authority,
                        };
                        match request_authority(&handover_stream, &authority_transfer_id) {
                            Ok(authority) => {
                                validate_authority(&authority, &expected)
                                    .map_err(|error| format!("Predecessor authority rejected: {error}"))?;
                                Ok(Some(authority))
                            }
                            // A pre-plan predecessor hangs up instead of answering; depending on
                            // timing that is EOF on the reply or EPIPE/ECONNRESET on the request.
                            Err(error) if predecessor_closed_without_authority(&error) => {
                                tracing::info!(
                                    %error,
                                    "Predecessor sent no authority (pre-plan daemon); taking the locks after commit"
                                );
                                Ok(None)
                            }
                            Err(error) => Err(format!("Predecessor authority transfer failed: {error}")),
                        }
                    })
                    .await
                    .map_err(|error| format!("Authority worker panicked: {error}"))??;
                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                    crate::daemon::qa_producers::note_authority_stage(
                        "authority-validated",
                        &self.handover_manager,
                    )
                    .await;

                    // The last pre-commit check: an expired request never commits.
                    if let Some(expires) = handover_expires_at {
                        crate::daemon::handover_request::pre_commit_remaining(
                            expires,
                            crate::daemon::owner_admission::unix_now_ms(),
                        )
                        .inspect_err(|_| expired_before_commit = true)?;
                    }
                    commit_started = true;
                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                    crate::daemon::qa_producers::note_authority_stage("commit-sent", &self.handover_manager).await;
                    let commit_started_at = tokio::time::Instant::now();
                    let commit_resp = legacy_peer
                        .send_request(&DaemonRequest::CommitHandover {
                            legacy_socket_path: None,
                        })
                        .await
                        .map_err(|e| format!("CommitHandover request failed: {e}"))?;
                    if !matches!(commit_resp, DaemonResponse::CommitHandoverOk) {
                        return Err(format!("CommitHandover failed: {commit_resp:?}"));
                    }
                    // The predecessor retires with exit 0 right after confirming; systemd must be
                    // supervising this successor before then, or it would stop tracking the daemon.
                    crate::daemon::supervisor_linux::notify_main_pid_ready();
                    let commit_latency_ms = commit_started_at.elapsed().as_millis() as u64;
                    Ok::<(usize, usize, usize, u64), String>((
                        offered,
                        accepted,
                        restorable,
                        commit_latency_ms,
                    ))
                }
                .await;

                match transfer_outcome {
                    Ok((offered, accepted, restorable, commit_latency_ms)) => {
                        if let Some(authority) = received_authority.take() {
                            if cross_rename {
                                inherited_legacy_compat = Some(self.install_cross_rename_authority(authority)?);
                                cross_rename_locks_installed = true;
                            } else {
                                inherited_canonical = Some(self.install_adopted_authority(authority)?);
                            }
                            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                            crate::daemon::qa_producers::note_authority_stage(
                                "authority-adopted",
                                &self.handover_manager,
                            )
                            .await;
                        }
                        tracing::info!(
                            accepted,
                            offered,
                            restorable,
                            commit_latency_ms,
                            "Handover committed after adopting the transferred sessions"
                        );
                        // QA fault (adoption todo 15): Q03b's failure case kills every adopted
                        // child right after commit. Read only under local-split-qa + QA marker env.
                        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                        if crate::daemon::qa_producers::kill_adopted_requested() {
                            crate::daemon::qa_producers::kill_adopted_children(
                                &self.terminal_service,
                                &adopted_sessions,
                            );
                        }
                        // This successor now owns the sessions, so the next login must start its
                        // generation, not the retiring predecessor's. A GUI-free upgrade has no GUI
                        // that would register it, and the candidate job was started without login
                        // autostart. A failure only costs the login registration, never the commit.
                        match crate::ipc::run_blocking(|| {
                            crate::daemon::supervisor::register_running_generation_for_login()
                                .map_err(crate::ipc::IpcError::internal)
                        })
                        .await
                        {
                            Ok(changed) => tracing::info!(changed, "Registered the committed generation for login"),
                            Err(error) => tracing::warn!(%error, "Could not register the committed generation for login"),
                        }
                        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                        {
                            if let Some(channel) = crate::daemon::qa_producers::channel().await {
                                crate::daemon::qa_producers::observe_reader_state(
                                    &self.terminal_service,
                                    &mut adopted_records,
                                );
                                crate::daemon::qa_producers::emit_handover_transfer_off_runtime(
                                    &channel,
                                    &transfer_id,
                                    &legacy_path,
                                    self.epoch(),
                                    crate::daemon::qa_producers::HandoverAccounting {
                                        offered,
                                        accepted,
                                        restorable,
                                    },
                                    commit_latency_ms,
                                    &adopted_records,
                                )
                                .await;
                            }
                        }
                        // The predecessor wrote a direct route for every SSH/paired session it
                        // kept (todo 8). Adopting it first makes the restore below skip them, so
                        // this daemon creates no controller for a session the predecessor serves.
                        if let Err(error) = self.session_router.adopt_routes_from_manifest().await {
                            tracing::error!(%error, "Could not adopt the retained-session routes after commit");
                        }
                        if let Some(legacy_manifest) = cross_rename
                            .then(|| legacy_path.with_file_name("handover_routes.json"))
                        {
                            if let Err(error) = self.session_router.adopt_routes_from_manifest_at(legacy_manifest).await {
                                tracing::error!(%error, "Could not adopt the pre-rename retained-session routes after commit");
                            }
                        }
                        if let Err(error) = self
                            .restore_remote_sessions_at(self.remote_sessions_path.clone())
                            .await
                        {
                            tracing::error!(%error, "Handover committed, but restoring the predecessor's SSH/remote sessions failed; they will reattach on the next daemon start");
                        }
                    }
                    Err(reason) => {
                        if commit_started {
                            let path = legacy_path.clone();
                            let decision = crate::ipc::run_blocking(move ||
                                HandoverManager::recorded_decision(&path).map_err(crate::ipc::IpcError::internal))
                                .await.map_err(|error| error.to_string())?;
                            if super::handover::resolve_commit(false, decision)
                                == super::handover::CommitResolution::Owner
                            {
                                tracing::info!("Recovered committed handover from recorded transaction decision");
                                if let Some(authority) = received_authority.take() {
                                    if cross_rename {
                                        inherited_legacy_compat = Some(self.install_cross_rename_authority(authority)?);
                                        cross_rename_locks_installed = true;
                                    } else {
                                        inherited_canonical = Some(self.install_adopted_authority(authority)?);
                                    }
                                }
                                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                crate::daemon::qa_producers::note_authority_stage("authority-recovered", &self.handover_manager).await;
                                // The predecessor committed and is retiring with exit 0; take over as
                                // the unit's main process exactly as the confirmed path does.
                                crate::daemon::supervisor_linux::notify_main_pid_ready();
                                // QA fault (adoption todo 15): the recovered path also makes this
                                // successor the committed owner, so Q03b's fault applies here too.
                                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                if crate::daemon::qa_producers::kill_adopted_requested() {
                                    crate::daemon::qa_producers::kill_adopted_children(
                                        &self.terminal_service,
                                        &adopted_sessions,
                                    );
                                }
                            } else {
                                // Not Retired: the predecessor never closed its authority. Closing
                                // these copies releases nothing it holds.
                                drop(received_authority.take());
                                release_after_unconfirmed_commit(async {
                                    for session_id in &adopted_sessions {
                                        self.terminal_service.pty_manager().relinquish_transferred_session(session_id)
                                            .await.map_err(|error| error.to_string())?;
                                        release_adopted_ownership(&self.session_metadata, std::slice::from_ref(session_id));
                                    }
                                    Ok(())
                                }, || async {
                                    legacy_peer.send_request(&DaemonRequest::AbortHandover).await
                                }).await?;
                                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                {
                                    if let Some(channel) = crate::daemon::qa_producers::channel().await {
                                        crate::daemon::qa_producers::observe_reader_state(
                                            &self.terminal_service,
                                            &mut adopted_records,
                                        );
                                        crate::daemon::qa_producers::emit_rollback_relinquishment_off_runtime(
                                            &channel,
                                            &transfer_id,
                                            &legacy_path,
                                            self.epoch(),
                                            &adopted_records,
                                            false,
                                            &reason,
                                        )
                                        .await;
                                    }
                                }
                                if handover_expires_at.is_some() {
                                    // The decision is not Retired: the predecessor kept its
                                    // authority and resumed; this candidate exits 0.
                                    tracing::warn!(%reason, ?decision, "Unconfirmed commit aborted; exiting 0");
                                    crate::daemon::supervisor_linux::notify_ready();
                                    return Ok(());
                                }
                                return Err(format!("{reason}; recorded decision: {decision:?}"));
                            }
                        } else {
                        drop(received_authority.take());
                        let rollback = rollback_transferred_readers(async {
                            for session_id in &adopted_sessions {
                                self.terminal_service.pty_manager().relinquish_transferred_session(session_id)
                                    .await.map_err(|failure| format!("{reason}; relinquish failed: {failure}; predecessor remains frozen"))?;
                                release_adopted_ownership(&self.session_metadata, std::slice::from_ref(session_id));
                            }
                            Ok(())
                        }, || async {
                            legacy_peer.send_request(&DaemonRequest::AbortHandover).await
                        }, || async {
                            let path = legacy_path.clone();
                            crate::ipc::run_blocking(move || HandoverManager::recorded_decision(&path)
                                .map_err(crate::ipc::IpcError::internal)).await.map_err(|error| error.to_string())
                        }).await;
                        let rolled_back = rollback.is_ok();
                        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                        crate::daemon::qa_producers::note_authority_stage("candidate-aborted", &self.handover_manager).await;
                        match rollback {
                            Ok(()) => {
                                tracing::warn!(
                                    %reason,
                                    "Handover transfer aborted; predecessor confirmed AbortHandover and resumed serving sessions"
                                );
                                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                {
                                    if let Some(channel) = crate::daemon::qa_producers::channel().await {
                                        crate::daemon::qa_producers::observe_reader_state(
                                            &self.terminal_service,
                                            &mut adopted_records,
                                        );
                                        crate::daemon::qa_producers::emit_rollback_relinquishment_off_runtime(
                                            &channel,
                                            &transfer_id,
                                            &legacy_path,
                                            self.epoch(),
                                            &adopted_records,
                                            true,
                                            &reason,
                                        )
                                        .await;
                                    }
                                }
                            }
                            other => {
                                tracing::error!(
                                    %reason,
                                    abort_result = ?other,
                                    "Handover transfer failed ({reason}) and predecessor AbortHandover returned {other:?}; exported sessions stay paused on the predecessor"
                                );
                            }
                        }
                        if crate::daemon::handover_request::candidate_exits_cleanly(
                            handover_expires_at.is_some(),
                            rolled_back,
                        ) {
                            tracing::warn!(%reason, expired_before_commit, "HandoverCandidate aborted before commit; predecessor resumed, exiting 0");
                            crate::daemon::supervisor_linux::notify_ready();
                            return Ok(());
                        }
                        return Err(reason);
                        }
                    }
                }
            } else {
                let sessions = legacy_peer.list_sessions().await?;
                let route = crate::daemon::manifest::HandoverRoute {
                    legacy_socket_path: legacy_path.clone(),
                    sessions,
                };
                crate::ipc::run_blocking(move || {
                    crate::daemon::manifest::HandoverManifest::update_at_path(
                        &crate::daemon::manifest::get_manifest_path(),
                        |manifest| manifest.add_or_update_route(route),
                    )
                    .map_err(|error| {
                        crate::ipc::IpcError::internal(format!(
                            "Failed to persist predecessor route before handover: {error}"
                        ))
                    })?;
                    Ok(())
                })
                .await
                .map_err(|error| error.to_string())?;

                let commit_resp = legacy_peer
                    .send_request(&DaemonRequest::CommitHandover {
                        legacy_socket_path: None,
                    })
                    .await?;
                if !matches!(commit_resp, DaemonResponse::CommitHandoverOk) {
                    return Err(format!("CommitHandover failed: {commit_resp:?}"));
                }
                crate::daemon::supervisor_linux::notify_main_pid_ready();
                self.session_router.add_legacy_peer(legacy_peer);
            }
        }

        #[cfg(unix)]
        let adopted_authority = inherited_canonical.is_some();
        #[cfg(not(unix))]
        let adopted_authority = false;
        #[cfg(unix)]
        let locks_installed = adopted_authority || cross_rename_locks_installed;
        #[cfg(not(unix))]
        let locks_installed = adopted_authority;
        // The ordinary acquire/bind path is skipped when authority was adopted: the inherited
        // lock fds were already installed by install_adopted_authority.
        if !locks_installed {
            let lock_files = acquire_daemon_locks(get_persistent_lock_path().as_deref(), &lock_path)?;
            self.handover_manager.set_lock_files(lock_files);
        }
        // Publish who owns the locks so launchers can tell this daemon from a reused pid, and
        // clear the launcher's pending-start marker. A failure only weakens admission to
        // Unknown (never a second start), so it is logged rather than fatal.
        let admission_paths = crate::daemon::owner_admission::AdmissionPaths::for_runtime();
        // A failed publication is retried every second on its own task (todo 8); serving does
        // not wait for it.
        Arc::clone(&self).spawn_owner_publication(admission_paths);

        // Clean up stale socket only after lock acquisition and safe ownership check.
        if !adopted_authority {
            remove_stale_socket_after_lock(&socket_path)?;
        }

        #[cfg(unix)]
        let listener = match inherited_canonical.take() {
            Some(inherited) => UnixListener::from_std(inherited)
                .map_err(|e| format!("Failed to serve the inherited daemon listener: {e}"))?,
            None => UnixListener::bind(&socket_path).map_err(|e| {
                format!(
                    "Failed to bind UDS socket at {}: {e}",
                    socket_path.display()
                )
            })?,
        };
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            self.handover_manager.register_authority_listener(
                crate::daemon::handover::AuthorityListenerRole::Canonical,
                listener.as_raw_fd(),
            )?;
        }

        #[cfg(not(unix))]
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind TCP listener on localhost: {e}"))?;

        #[cfg(not(unix))]
        {
            let port = listener
                .local_addr()
                .map_err(|e| format!("Failed to get local port: {e}"))?
                .port();
            // Publish the pair as one unit, in the order the reader depends on: the token is this
            // boot's only client credential and is on disk before the accept loop admits a
            // connection, and the port is the marker a client keys on, so it is published last.
            publish_transport_rendezvous(
                &socket_path,
                &get_transport_token_path(),
                port,
                &self.transport_token,
            )
            .map_err(|e| format!("Failed to publish daemon port and token: {e}"))?;
        }

        // Ensure 0600 mode
        validate_safe_ownership_and_type(&socket_path, RuntimeNodeKind::Socket)?;
        #[cfg(unix)]
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Failed to secure daemon socket: {error}"))?;
        self.note_published_endpoint(&socket_path);
        #[cfg(not(unix))]
        self.note_published_endpoint(&get_transport_token_path());

        tracing::info!("Ferryx daemon listening on {}", socket_path.display());

        // One release of compatibility: GUIs from before the rename dial /tmp/rorca-{uid}.
        #[cfg(unix)]
        Arc::clone(&self).spawn_legacy_socket_listener(inherited_legacy_compat.take());

        // control-v1: bound right after the data listener, only while this process holds the
        // daemon locks. A committed HandoverCandidate binds it here too: without it the next GUI
        // would see a pre-plan daemon and could never upgrade it again.
        self.bind_control_endpoint().await?;

        // Every start adopts every route in the manifest, handover or not (todo 8): routes are
        // direct, so a successor talks to each generation that still owns sessions over that
        // generation's own legacy socket. adopt_routes_from_manifest skips peers already added.
        self.session_router.adopt_routes_from_manifest().await?;
        // Routes a daemon from before the rename wrote live in its own runtime dir.
        if let Some(legacy_manifest) = get_legacy_runtime_dir().map(|dir| dir.join("handover_routes.json")) {
            // Only a pre-rename daemon writes this file; a bad one must not stop this start.
            if let Err(error) = self.session_router.adopt_routes_from_manifest_at(legacy_manifest).await {
                tracing::warn!(%error, "Could not adopt the pre-rename handover routes");
            }
        }

        if handover_from.is_none() {
            self.restore_remote_sessions_at(self.remote_sessions_path.clone())
                .await?;
        }

        // Both platform listeners share this call site: unix binds the socket path, every other
        // platform binds a loopback TCP port. This runs before the readiness signal below: a
        // client released by that signal can spawn a pane immediately, and the pane reads its
        // agent-state endpoint from disk as it spawns, so a listener started afterwards leaves
        // that pane with no endpoint for its whole lifetime.
        let agent_state_ingress = self.spawn_agent_state_listener();
        self.spawn_foreground_observer();
        self.spawn_paired_host_probe(crate::paired_host::client::PROBE_INTERVAL);
        self.spawn_paired_host_route_forwarder();
        crate::daemon::agent_extension::install_agent_state_extension_if_enabled(
            &self.agent_integration,
        );

        // The ingress outcome is observed and recorded before readiness is released, so a boot
        // that announces itself ready without a usable ingress is reported rather than inferred.
        self.settle_agent_state_ingress(agent_state_ingress, || {
            if let Some(tx) = ready_tx {
                let _ = tx.send(());
            }
        });

        let persisted_remote_config = self.remote_state.config.read().clone();
        if persisted_remote_config.mode != RemoteNetworkMode::Off {
            if let Err(error) = self.handle_remote_configure(persisted_remote_config).await {
                tracing::warn!("Failed to restore daemon remote gateway listener: {error}");
            }
        }

        loop {
            // Public accepts stop while a commit is decided and forever once this daemon has
            // handed its listener to a successor (todo 8); the legacy socket is not gated. Once
            // handed over, this copy is closed: a draining predecessor that kept it left the
            // path connectable with nobody accepting after the successor retired (run-int1
            // Q08b), and it keeps serving its retained sessions on the legacy socket until
            // check_retirement_if_empty exits the process.
            let Some(accepted) = self.handover_manager.gated_accept(|| listener.accept()).await else {
                drop(listener);
                std::future::pending::<()>().await;
                unreachable!("pending never resolves");
            };
            match accepted {
                Ok((stream, _)) => {
                    self.handover_manager
                        .note_accept(crate::daemon::handover::AuthorityListenerRole::Canonical);
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        server.handle_client(stream).await;
                    });
                }
                Err(e) => {
                    tracing::error!("Daemon accept error: {e}");
                    break;
                }
            }
        }

        Ok(())
    }

    fn resource_probes(&self) -> Vec<crate::daemon::resource_usage::SessionProbe> {
        self.terminal_service
            .list_sessions()
            .into_iter()
            .filter_map(|session_id| {
                let session = self.terminal_service.get_session(&session_id)?;
                Some(crate::daemon::resource_usage::SessionProbe {
                    session_id,
                    pid: session.pid(),
                    worktree_path: session
                        .worktree_path()
                        .map(|path| path.to_string_lossy().into_owned()),
                })
            })
            .collect()
    }

    pub async fn handle_client<S>(self: Arc<Self>, stream: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        // QaFixtureKind::DaemonListenerStall (QA builds only): accepted, handshake not answered until released.
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        crate::ipc::qa_barrier::daemon_listener_stall_gate().await;
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        let mut abort_rx = self.handover_manager.subscribe_client_abort();
        // Set once the connection has presented this boot's transport token. Unix inherits the
        // socket's ownership boundary and never needs the check.
        #[cfg(not(unix))]
        let mut transport_authenticated = false;
        #[cfg(not(unix))]
        const TRANSPORT_REJECTION: &str =
            "Daemon connection rejected: transport token missing or invalid";

        loop {
            tokio::select! {
                _ = abort_rx.recv() => {
                    break;
                }
                read_res = reader.read_line(&mut line) => {
                    match read_res {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let req: Result<DaemonRequest, _> = serde_json::from_str(line.trim());
                    // The loopback transport has no filesystem ownership boundary, so the first
                    // frame must carry this boot's token before any request is dispatched. A
                    // rejected connection is closed rather than answered, so a local process
                    // that can only read `daemon.port` cannot drive the daemon.
                    #[cfg(not(unix))]
                    if !transport_authenticated {
                        let presented = transport_token_from_line(&line);
                        if !transport_token_matches(&self.transport_token, presented.as_deref()) {
                            tracing::warn!("{}", TRANSPORT_REJECTION);
                            let mut rejection = serde_json::to_string(&DaemonResponse::Error {
                                message: TRANSPORT_REJECTION.to_string(),
                                code: Some("TRANSPORT_UNAUTHORIZED".to_string()),
                                details: None,
                            })
                            .unwrap_or_else(|_| "{\"type\":\"error\"}".to_string());
                            rejection.push('\n');
                            let _ = write_half.write_all(rejection.as_bytes()).await;
                            let _ = write_half.flush().await;
                            break;
                        }
                        transport_authenticated = true;
                    }
                    line.clear();
                    let should_check_retirement = matches!(
                        req.as_ref(),
                        Ok(DaemonRequest::Close { .. }) | Ok(DaemonRequest::Hibernate { .. }) | Ok(DaemonRequest::CommitHandover { .. })
                    );
                    #[cfg(test)]
                    let is_close = matches!(req.as_ref(), Ok(DaemonRequest::Close { .. }));
                    // Every one-shot request holds a RetirementGuard until its response is written,
                    // flushed and the retirement check below has run, so a retiring daemon never
                    // exits with this response unwritten (control-v1, todo 15). Long-lived streams
                    // are excluded: they would keep a retiring daemon alive for as long as a
                    // subscriber stays connected. A Retired daemon refuses the guard; the request
                    // is still answered as before.
                    let holds_retirement_guard = !matches!(
                        req.as_ref(),
                        Ok(DaemonRequest::Attach { .. })
                            | Ok(DaemonRequest::MachineGateway)
                            | Ok(DaemonRequest::MachineMetadataSubscribe { .. })
                            | Ok(DaemonRequest::SubscribeRemoteEvents)
                            | Ok(DaemonRequest::SubscribeDag { .. })
                            // The handover exchange itself never holds a guard: the commit waits
                            // for every guarded request to finish, so CommitHandover holding one
                            // would wait on itself.
                            | Ok(DaemonRequest::PrepareHandover)
                            | Ok(DaemonRequest::TransferSessions { .. })
                            | Ok(DaemonRequest::CommitHandover { .. })
                            | Ok(DaemonRequest::AbortHandover)
                    );
                    // While a commit is decided (HandoverStatus::Committing) a new request is not
                    // admitted: it is answered HANDOVER_BUSY, retryable, and never half-served.
                    let mut busy_refusal = None;
                    let retirement_guard = if holds_retirement_guard {
                        match self
                            .handover_manager
                            .retain_request(Arc::clone(&self.terminal_service))
                        {
                            Ok(guard) => Some(guard),
                            Err(refusal) if super::handover::is_spawn_gate_busy(&refusal) => {
                                busy_refusal = Some(DaemonResponse::Error {
                                    code: Some(super::handover::HANDOVER_BUSY.into()),
                                    message: refusal,
                                    details: None,
                                });
                                None
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    let resp = match req {
                _ if busy_refusal.is_some() => busy_refusal.take().expect("checked above"),
                // A committed predecessor never serves a local session it transferred: the
                // successor is that session's only owner (todo 8).
                Ok(ref request) if request_session_id(request)
                    .is_some_and(|id| self.handover_manager.session_not_owned(id)) =>
                {
                    let session_id = request_session_id(request).unwrap_or_default().to_owned();
                    DaemonResponse::Error {
                        code: Some(super::handover::SESSION_NOT_OWNED.into()),
                        message: format!("Session '{session_id}' was transferred to the successor daemon"),
                        details: Some(serde_json::json!({ "sessionId": session_id })),
                    }
                }
                Ok(DaemonRequest::Handshake { version, .. }) => {
                    if self.handover_manager.status() == super::handover::HandoverStatus::Retired {
                        DaemonResponse::Error {
                            code: Some("DAEMON_RETIRED".into()),
                            message: "Daemon ownership transferred to successor".into(),
                            details: None,
                        }
                    } else if version != DAEMON_PROTOCOL_VERSION {
                        DaemonResponse::ProtocolMismatch {
                            expected_version: DAEMON_PROTOCOL_VERSION,
                            received_version: version,
                        }
                    } else {
                        DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: self.epoch,
                            binary_path: self.binary_path.clone(),
                            binary_mtime_ms: self.binary_mtime_ms,
                            daemon_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                            capabilities: vec![
                                crate::daemon::protocol::LOCAL_SPLIT_LIFECYCLE_CAPABILITY.to_string(),
                                crate::daemon::control::CONTROL_CAPABILITY.to_string(),
                            ],
                            admission_time_unix_ms: Some(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0),
                            ),
                        }
                    }
                }
                Ok(DaemonRequest::Ping) => DaemonResponse::Pong,
                Ok(DaemonRequest::ResourceUsage) => {
                    let probes = self.resource_probes();
                    let sampler = Arc::clone(&self.resource_sampler);
                    let runtime_dir = get_runtime_dir();
                    match crate::ipc::run_blocking(move || Ok(sampler.sample(&probes, &runtime_dir))).await {
                        Ok(snapshot) => DaemonResponse::ResourceUsageOk { snapshot },
                        Err(error) => daemon_error(format!("Resource sampling failed: {error}")),
                    }
                }
                Ok(DaemonRequest::SshPassword { host, password }) => {
                    let result = crate::ipc::run_blocking(move || match password {
                        Some(password) => crate::ssh::password::set(&host, password),
                        None => crate::ssh::password::clear(&host),
                    }).await;
                    match result {
                        Ok(()) => DaemonResponse::Pong,
                        Err(error) => DaemonResponse::WorktreeError { error },
                    }
                }
                Ok(DaemonRequest::RemoteSessionDetails { session_id }) => {
                    let local_details = self.terminal_service.remote().details(&session_id);
                    // A rolling handover leaves live remote sessions with the draining
                    // predecessor (restore_remote_sessions_at skips them on purpose), so the
                    // local runtime legitimately has no entry. Answering `details: null` would
                    // make the GUI treat the session as missing and respawn it; route the query
                    // to the daemon that still owns the controller instead.
                    let routed = match local_details.is_none()
                        .then(|| self.session_service.router().find_legacy_peer_for_session(&session_id))
                        .flatten()
                    {
                        Some(peer) => match peer.send_request(&DaemonRequest::RemoteSessionDetails { session_id: session_id.clone() }).await {
                            Ok(response) => match &response {
                                // The peer does not know this session either: fall back to the
                                // local answer (including the legacy direct-SSH classification).
                                // Only the structured discriminator counts; message text must
                                // never reclassify an unrelated peer error as "session absent".
                                DaemonResponse::Error { code, .. }
                                    if code.as_deref() == Some("SESSION_NOT_FOUND") => None,
                                _ => Some(response),
                            },
                            // A transport failure is not evidence of absence; never degrade it
                            // into `details: null`.
                            Err(error) => Some(daemon_error(format!("Legacy peer remote session details failed: {error}"))),
                        },
                        None => None,
                    };
                    match routed {
                        Some(response) => response,
                        None => {
                            let legacy_direct_ssh = local_details.is_none() && self.session_metadata.read().get(&session_id).is_some_and(|m| crate::ssh::projects::is_remote(&m.workspace_id));
                            DaemonResponse::RemoteSessionDetailsOk { details: local_details, legacy_direct_ssh }
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaCreateRemoteSession { config, params, client_request_id }) => {
                    if !active_transport_qa_profile() {
                        DaemonResponse::Error {
                            message: "Transport QA session creation requires an isolated QA runtime and data directory".into(),
                            code: Some("QA_ISOLATION_REQUIRED".into()),
                            details: None,
                        }
                    } else {
                    match self.terminal_service.remote()
                        .create_for_transport_qa(config, params, client_request_id).await
                    {
                        Ok((descriptor, details)) => DaemonResponse::QaCreateRemoteSessionOk { descriptor, details },
                        Err(failure) => DaemonResponse::RemoteSessionError { failure },
                    }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaRemoteOwnerMutation { session_id, operation, generation, data }) => {
                    if !active_transport_qa_profile() {
                        DaemonResponse::Error { message: "Transport QA owner mutation requires an isolated QA runtime".into(), code: Some("QA_ISOLATION_REQUIRED".into()), details: None }
                    } else {
                        let remote = self.terminal_service.remote();
                        let result = match operation.as_str() {
                            "write" => match generation {
                                Some(generation) => remote.transport_qa_write(&session_id, generation, data).await,
                                None => Err(crate::terminal::remote::RemoteFailure { kind: crate::terminal::remote::RemoteFailureKind::Protocol, message: "write requires generation".into() }),
                            },
                            "close" => remote.transport_qa_close(&session_id).await,
                            "recover" => remote.transport_qa_recover(&session_id),
                            _ => Err(crate::terminal::remote::RemoteFailure { kind: crate::terminal::remote::RemoteFailureKind::Protocol, message: "unsupported QA owner mutation".into() }),
                        };
                        match result {
                            Ok(()) => match remote.details(&session_id) {
                                Some(details) => DaemonResponse::QaRemoteOwnerAuditOk {
                                    events: remote.transport_qa_audit(&session_id), details,
                                },
                                None => daemon_error("QA session details disappeared"),
                            },
                            Err(failure) => DaemonResponse::RemoteSessionError { failure },
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaRemoteOwnerAudit { session_id }) => {
                    if !active_transport_qa_profile() {
                        DaemonResponse::Error { message: "Transport QA owner audit requires an isolated QA runtime".into(), code: Some("QA_ISOLATION_REQUIRED".into()), details: None }
                    } else {
                        match self.terminal_service.remote().details(&session_id) {
                            Some(details) => DaemonResponse::QaRemoteOwnerAuditOk {
                                events: self.terminal_service.remote().transport_qa_audit(&session_id), details,
                            },
                            None => daemon_error("QA session not found"),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaRemoteOwnerWaitState { session_id, state }) => {
                    if !active_transport_qa_profile() {
                        DaemonResponse::Error { message: "Transport QA owner state wait requires an isolated QA runtime".into(), code: Some("QA_ISOLATION_REQUIRED".into()), details: None }
                    } else {
                        match self.terminal_service.remote().transport_qa_wait_state(&session_id, state).await {
                            Ok(details) => DaemonResponse::QaRemoteOwnerWaitStateOk { details },
                            Err(failure) => DaemonResponse::RemoteSessionError { failure },
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaRemoteOwnerWaitRefusals { session_id, count }) => {
                    if !active_transport_qa_profile() {
                        DaemonResponse::Error { message: "Transport QA refusal wait requires an isolated QA runtime".into(), code: Some("QA_ISOLATION_REQUIRED".into()), details: None }
                    } else {
                        match self.terminal_service.remote().transport_qa_wait_refusals(&session_id, count).await {
                            Ok(details) => DaemonResponse::QaRemoteOwnerWaitRefusalsOk { details },
                            Err(failure) => DaemonResponse::RemoteSessionError { failure },
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaRemoteOwnerReleaseFaultRoute { session_id }) => {
                    if !active_transport_qa_profile() {
                        DaemonResponse::Error { message: "Transport QA fault route requires an isolated QA runtime".into(), code: Some("QA_ISOLATION_REQUIRED".into()), details: None }
                    } else {
                        match self.terminal_service.remote().transport_qa_release_fault_route(&session_id) {
                            Ok(details) => DaemonResponse::QaRemoteOwnerReleaseFaultRouteOk { details },
                            Err(failure) => DaemonResponse::RemoteSessionError { failure },
                        }
                    }
                }
                Ok(DaemonRequest::MachineGateway) => {
                    if let Err(error) = write_half.write_all(b"{\"type\":\"machineGatewayOk\"}\n").await {
                        tracing::warn!(%error, "Legacy machine gateway admission failed");
                        return;
                    }
                    Box::pin(machine_gateway::serve(tokio::io::join(reader, write_half), self.remote_state().clone())).await;
                    return;
                }
                Ok(DaemonRequest::MachineMetadataSubscribe { target }) => {
                    if let Err(error) = self.session_service.serve_metadata(tokio::io::join(reader, write_half), target).await {
                        tracing::debug!(%error, "Owner metadata subscription ended");
                    }
                    return;
                }
                Ok(DaemonRequest::MachineSessionMetadata { target, report }) => {
                    match Box::pin(self.session_service.validate_machine_agent_report(target, report)).await {
                        Ok(session) => DaemonResponse::MachineSessionDetailOk { detail: crate::remote::machine_protocol::SessionDetail::Running { session } },
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::MachineSessionDetail { session_id }) => {
                    match Box::pin(self.session_service.machine_detail_routed(&session_id, crate::scoped_contracts::Epoch(self.epoch))).await {
                        Ok(detail) => DaemonResponse::MachineSessionDetailOk { detail },
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::RetryRemoteSession { session_id }) => {
                    // Retry must reach the daemon that owns the controller: during a rolling
                    // handover that is the draining predecessor, not this runtime.
                    match (!self.terminal_service.remote().contains(&session_id))
                        .then(|| self.session_service.router().find_legacy_peer_for_session(&session_id))
                        .flatten()
                    {
                        Some(peer) => match peer.send_request(&DaemonRequest::RetryRemoteSession { session_id: session_id.clone() }).await {
                            Ok(response) => response,
                            Err(error) => daemon_error(format!("Legacy peer remote session retry failed: {error}")),
                        },
                        None => match self.validate_session_ssh_target(&session_id).await {
                            Err(e) => DaemonResponse::Error {
                                message: e.message,
                                code: Some(e.code.to_string()),
                                details: e.details,
                            },
                            Ok(()) => match self.terminal_service.remote().retry(&session_id) {
                                Ok(()) => DaemonResponse::RetryRemoteSessionOk,
                                Err(failure) => DaemonResponse::RemoteSessionError { failure },
                            }
                        }
                    }
                },
                Ok(DaemonRequest::RemoteWrite { session_id, generation, data }) if self.serves_paired_here(&session_id) => {
                    match self.terminal_service.write_input_operation(&session_id, generation, data) {
                        Ok(op) => match op.await {
                            Ok(()) => if qa_hold_write_ack(&session_id, &mut reader).await { DaemonResponse::WriteOk } else { break },
                            Err(e) => daemon_error(e.to_string()),
                        },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::RemoteResize { session_id, generation, cols, rows }) if self.serves_paired_here(&session_id) => {
                    self.record_desktop_geometry(&session_id, cols, rows);
                    match self.terminal_service.resize_operation(&session_id, generation, cols, rows) {
                        Ok(op) => match op.await { Ok(()) => DaemonResponse::ResizeOk, Err(e) => daemon_error(e.to_string()) },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::RemoteWrite { session_id, generation, data }) => {
                    match (!self.terminal_service.remote().contains(&session_id))
                        .then(|| self.session_service.router().find_legacy_peer_for_session(&session_id))
                        .flatten()
                    {
                        Some(peer) => match peer.send_request(&DaemonRequest::RemoteWrite { session_id, generation, data }).await {
                            Ok(response) => response,
                            Err(error) => daemon_error(format!("Legacy peer remote write failed: {error}")),
                        },
                        None => match self.validate_session_ssh_target(&session_id).await {
                            Err(e) => DaemonResponse::Error {
                                message: e.message,
                                code: Some(e.code.to_string()),
                                details: e.details,
                            },
                            Ok(()) => match self.terminal_service.remote().write(&session_id, generation, data) {
                                Ok(op) => match op.await {
                                    Ok(()) => if qa_hold_write_ack(&session_id, &mut reader).await { DaemonResponse::WriteOk } else { break },
                                    Err(failure) => DaemonResponse::RemoteSessionError { failure },
                                },
                                Err(failure) => DaemonResponse::RemoteSessionError { failure },
                            }
                        }
                    }
                }
                Ok(DaemonRequest::RemoteResize { session_id, generation, cols, rows }) => {
                    self.record_desktop_geometry(&session_id, cols, rows);
                    match (!self.terminal_service.remote().contains(&session_id))
                        .then(|| self.session_service.router().find_legacy_peer_for_session(&session_id))
                        .flatten()
                    {
                        Some(peer) => match peer.send_request(&DaemonRequest::RemoteResize { session_id, generation, cols, rows }).await {
                            Ok(response) => response,
                            Err(error) => daemon_error(format!("Legacy peer remote resize failed: {error}")),
                        },
                        None => match self.validate_session_ssh_target(&session_id).await {
                            Err(e) => DaemonResponse::Error {
                                message: e.message,
                                code: Some(e.code.to_string()),
                                details: e.details,
                            },
                            Ok(()) => match self.terminal_service.remote().resize(&session_id, generation, cols, rows) {
                                Ok(op) => match op.await { Ok(()) => DaemonResponse::ResizeOk, Err(failure) => DaemonResponse::RemoteSessionError { failure } },
                                Err(failure) => DaemonResponse::RemoteSessionError { failure },
                            }
                        }
                    }
                },
                Ok(DaemonRequest::CreateWorktree { workspace_id, worktree, base_ref }) => {
                    match crate::remote::workspace_api::worktrees::owner_mutation(self.remote_state().clone(), workspace_id, worktree, base_ref, None).await {
                        Ok(response) => response,
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::DeleteWorktree { workspace_id, worktree, delete_branch, destructive }) => {
                    match crate::remote::workspace_api::worktrees::owner_mutation(self.remote_state().clone(), workspace_id, worktree, None, Some((delete_branch, destructive))).await {
                        Ok(response) => response,
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::SyncDesktopLocalProjects { projects }) => {
                    let service = Arc::clone(&self.session_service.workspace_service);
                    match crate::ipc::run_blocking(move || service.sync_desktop_local_projects(projects)
                        .map_err(crate::ipc::IpcError::internal)).await {
                        Ok(()) => DaemonResponse::RegisterWorkspaceOk,
                        Err(error) => daemon_error(error.to_string()),
                    }
                }
                Ok(DaemonRequest::RegisterWorkspace {
                    workspace_id,
                    repo_root,
                }) => match {
                    let service = Arc::clone(&self.session_service.workspace_service);
                    crate::ipc::run_blocking(move || service.register(&workspace_id, &repo_root)
                        .map_err(crate::ipc::IpcError::internal)).await
                } {
                    Ok(()) => DaemonResponse::RegisterWorkspaceOk,
                    Err(e) => daemon_error(e.to_string()),
                },
                Ok(DaemonRequest::UnregisterWorkspace { workspace_id }) => {
                    match self.handle_unregister_workspace(&workspace_id).await {
                        Ok(()) => DaemonResponse::UnregisterWorkspaceOk,
                        Err(e) => daemon_error(e),
                    }
                }
                Ok(DaemonRequest::Spawn {
                    client_request_id,
                    workspace_id,
                    worktree,
                    cwd,
                    cols,
                    rows,
                    shell,
                    startup,
                    local_split,
                    session_id: requested_session_id,
                }) => {
                    if let Some(envelope) = local_split {
                        if startup.is_some() {
                            daemon_error("Local split does not accept agent startup")
                        } else {
                            match self.session_service.create_split(&client_request_id,
                                &workspace_id, worktree, cwd, cols, rows, shell, envelope, self.epoch).await {
                                Ok(crate::daemon::protocol::SplitOperationResult::Created {
                                    session_id, daemon_epoch, session, ..
                                }) => DaemonResponse::SpawnOk { session_id, epoch: daemon_epoch, session },
                                Ok(operation) => DaemonResponse::SpawnOperationOk { operation },
                                Err(error) => DaemonResponse::Error { message: error.message,
                                    code: Some(error.code.to_string()), details: error.details },
                            }
                        }
                    } else {
                    let res = self.session_service
                        .handle_spawn_requested(
                            &client_request_id,
                            &workspace_id,
                            worktree,
                            cwd,
                            cols,
                            rows,
                            shell,
                            startup,
                            requested_session_id,
                            #[cfg(test)]
                            self.helper_home.clone(),
                        )
                        .await;
                    match res {
                        Ok(session_id) => match self.handle_describe_session(&session_id) {
                            DaemonResponse::DescribeSessionOk { session, .. } => {
                                DaemonResponse::SpawnOk {
                                    session_id,
                                    epoch: self.epoch,
                                    session,
                                }
                            }
                            DaemonResponse::Error { message, .. } => daemon_error(message),
                            _ => daemon_error("Failed to describe spawned session".to_string(),),
                        },
                        Err(SpawnError::AgentSessionConflict {
                            agent_type,
                            provider_key,
                            provider_id,
                            existing_session_id,
                        }) => DaemonResponse::AgentSessionConflict {
                            agent_type,
                            provider_key,
                            provider_id,
                            existing_session_id,
                        },
                        Err(SpawnError::InvalidAgentResume(message)) => {
                            DaemonResponse::AgentResumeInvalid { message }
                        }
                        Err(SpawnError::Structured(error)) => DaemonResponse::Error {
                            message: error.message,
                            code: Some(error.code.to_string()),
                            details: error.details,
                        },
                        Err(e) => daemon_error(e.to_string(),),
                    }
                    }
                }
                Ok(DaemonRequest::SpawnOperationStatus { client_request_id, origin_epoch, expires_at_unix_ms }) => {
                    match self.session_service.split_status(&client_request_id, origin_epoch, self.epoch, expires_at_unix_ms).await {
                        Ok(operation) => DaemonResponse::SpawnOperationOk { operation },
                        Err(error) => daemon_error(error.message),
                    }
                }
                Ok(DaemonRequest::CancelSpawnOperation { client_request_id, origin_epoch, expires_at_unix_ms: _ }) => {
                    match tokio::time::timeout(Duration::from_millis(2500),
                        self.session_service.cancel_split(&client_request_id, origin_epoch, self.epoch)).await {
                        Ok(Ok(operation)) => DaemonResponse::SpawnOperationOk { operation },
                        Ok(Err(error)) => daemon_error(error.message),
                        Err(_) => DaemonResponse::SpawnOperationOk { operation:
                            crate::daemon::protocol::SplitOperationResult::Unknown {
                                reason: crate::daemon::protocol::SplitUnknownReason::PublicationUncertain,
                            } },
                    }
                }
                Ok(DaemonRequest::DetectManualSsh { session_id }) => {
                    if let Some(session) = self.terminal_service.get_session(&session_id) {
                        let detected = crate::ipc::run_blocking(move || {
                            Ok(crate::terminal::manual_ssh::detect(&session))
                        })
                        .await;
                        match detected {
                            Ok(Ok(ssh)) => DaemonResponse::ManualSshOk { ssh },
                            Ok(Err(error)) => DaemonResponse::Error {
                                message: error.message(),
                                code: Some(error.code().to_string()),
                                details: None,
                            },
                            Err(error) => DaemonResponse::Error {
                                message: error.message,
                                code: Some(crate::terminal::manual_ssh::CODE_IO.to_string()),
                                details: None,
                            },
                        }
                    } else if self.session_router.is_local_session(&session_id) {
                        // Remote/paired sessions have registered upload routes of their own.
                        DaemonResponse::Error {
                            message: "Session is not a local terminal".into(),
                            code: Some(crate::terminal::manual_ssh::CODE_UNSUPPORTED.into()),
                            details: None,
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.send_request(&DaemonRequest::DetectManualSsh { session_id: session_id.clone() }).await {
                            Ok(response) => response,
                            Err(error) => daemon_error(format!("Legacy peer manual SSH detection failed: {error}")),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_detect_manual_ssh")
                    }
                }
                Ok(DaemonRequest::DescribeSession { session_id }) => {
                    self.describe_session_identity(&session_id).await
                }
                Ok(DaemonRequest::DiscoverAgentSession {
                    session_id,
                    agent_type,
                }) => {
                    if self.session_router.is_local_session(&session_id) {
                        let maybe_pid = self
                            .terminal_service
                            .get_session(&session_id)
                            .and_then(|session| session.pid());
                        let provider_session_id = match maybe_pid {
                            Some(pid) => {
                                let agent_type = agent_type.clone();
                                tokio::task::spawn_blocking(move || {
                                    crate::ipc::agents::discover_agent_session_id(pid, &agent_type)
                                })
                                .await
                                .ok()
                                .flatten()
                            }
                            None => None,
                        };
                        DaemonResponse::DiscoverAgentSessionOk {
                            provider_session_id,
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.discover_agent_session(&session_id, &agent_type).await {
                            Ok(provider_session_id) => DaemonResponse::DiscoverAgentSessionOk { provider_session_id },
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_discover_agent")
                    }
                }
                Ok(DaemonRequest::ResetAgentState { session_id }) => {
                    if self.session_router.is_local_session(&session_id) || self.agent_states.current(&session_id).is_some() {
                        self.agent_states.release_manual(&session_id);
                        DaemonResponse::ResetAgentStateOk
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        let peer_call = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            peer.reset_agent_state(&session_id),
                        )
                        .await;
                        match peer_call {
                            Ok(Ok(())) => {
                                self.agent_states.release_manual(&session_id);
                                DaemonResponse::ResetAgentStateOk
                            }
                            Ok(Err(message)) => daemon_error(message),
                            Err(_) => {
                                tracing::warn!(session_id, "peer reset_agent_state timed out; releasing locally");
                                self.agent_states.release_manual(&session_id);
                                DaemonResponse::ResetAgentStateOk
                            }
                        }
                    } else {
                        self.agent_states.release_manual(&session_id);
                        DaemonResponse::ResetAgentStateOk
                    }
                }
                Ok(DaemonRequest::Write { session_id, data }) if self.serves_paired_here(&session_id) => {
                    match self.terminal_service.write_input_operation(&session_id, 0, data) {
                        Ok(pending) => match pending.await {
                            Ok(()) => {
                                crate::remote::server::agent_control_service::note_human_keystrokes(&session_id);
                                DaemonResponse::WriteOk
                            }
                            Err(e) => daemon_error(e.to_string()),
                        },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::Resize { session_id, cols, rows }) if self.serves_paired_here(&session_id) => {
                    match self.terminal_service.resize_operation(&session_id, 0, cols, rows) {
                        Ok(pending) => match pending.await {
                            Ok(()) => DaemonResponse::ResizeOk,
                            Err(e) => daemon_error(e.to_string()),
                        },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::Write { session_id, data }) => {
                    #[cfg(feature = "local-split-qa")]
                    let tok = super::qa_producers::record_daemon_mutation_ingress(&session_id, "write", &data);
                    if self.session_router.is_local_session(&session_id) {
                        match self.write_session_input(&session_id, data).await {
                            Ok(()) => {
                                #[cfg(feature = "local-split-qa")]
                                super::qa_producers::record_daemon_mutation_delivery(&session_id, "write", &tok);
                                crate::remote::server::agent_control_service::note_human_keystrokes(&session_id);
                                DaemonResponse::WriteOk
                            }
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.write_input(&session_id, &data).await {
                            Ok(()) => {
                                #[cfg(feature = "local-split-qa")]
                                super::qa_producers::record_daemon_mutation_delivery(&session_id, "write", &tok);
                                crate::remote::server::agent_control_service::note_human_keystrokes(&session_id);
                                DaemonResponse::WriteOk
                            }
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_write")
                    }
                }
                Ok(DaemonRequest::Resize {
                    session_id,
                    cols,
                    rows,
                }) => {
                    self.record_desktop_geometry(&session_id, cols, rows);
                    #[cfg(feature = "local-split-qa")]
                    let tok = super::qa_producers::record_daemon_mutation_ingress(&session_id, "resize", format!("{cols}x{rows}").as_bytes());
                    if self.session_router.is_local_session(&session_id) {
                        match self.resize_session(&session_id, cols, rows).await {
                            Ok(()) => {
                                #[cfg(feature = "local-split-qa")]
                                super::qa_producers::record_daemon_mutation_delivery(&session_id, "resize", &tok);
                                DaemonResponse::ResizeOk
                            }
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.resize(&session_id, cols, rows).await {
                            Ok(()) => {
                                #[cfg(feature = "local-split-qa")]
                                super::qa_producers::record_daemon_mutation_delivery(&session_id, "resize", &tok);
                                DaemonResponse::ResizeOk
                            }
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_resize")
                    }
                }
                Ok(DaemonRequest::Signal { session_id, signal }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.terminal_service.signal(&session_id, signal) {
                            Ok(()) => DaemonResponse::SignalOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.signal(&session_id, signal).await {
                            Ok(()) => DaemonResponse::SignalOk,
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_signal")
                    }
                }
                Ok(DaemonRequest::Close { session_id }) => {
                    #[cfg(feature = "local-split-qa")]
                    let tok = super::qa_producers::record_daemon_mutation_ingress(&session_id, "close", b"close");
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_close(&session_id).await {
                            Ok(()) => {
                                #[cfg(feature = "local-split-qa")]
                                super::qa_producers::record_daemon_mutation_delivery(&session_id, "close", &tok);
                                DaemonResponse::CloseOk
                            }
                            Err(crate::terminal::PtyError::SessionNotFound(_)) => daemon_session_not_found(&session_id, "daemon_close"),
                            Err(e) => daemon_error(e),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        match peer.close(&session_id).await {
                            Ok(()) => {
                                #[cfg(feature = "local-split-qa")]
                                super::qa_producers::record_daemon_mutation_delivery(&session_id, "close", &tok);
                                self.agent_states.remove(&session_id);
                                DaemonResponse::CloseOk
                            }
                            Err(message) => daemon_error(message),
                        }
                    } else {
                        // Idempotent close: closing an already closed or non-existent session is a success.
                        #[cfg(feature = "local-split-qa")]
                        super::qa_producers::record_daemon_mutation_delivery(&session_id, "close", &tok);
                        DaemonResponse::CloseOk
                    }
                }
                Ok(DaemonRequest::Hibernate { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_hibernate(&session_id).await {
                            Ok(()) => DaemonResponse::HibernateOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else {
                        // Idempotent hibernate: a process that is already absent needs no work.
                        DaemonResponse::HibernateOk
                    }
                }
                Ok(DaemonRequest::Suspend { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_suspend(&session_id).await {
                            Ok(()) => DaemonResponse::SuspendOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if self.session_router.find_legacy_peer_for_session(&session_id).is_some() {
                        daemon_error("Session suspend is not supported for peer sessions".to_string(),)
                    } else {
                        daemon_session_not_found(&session_id, "daemon_suspend")
                    }
                }
                Ok(DaemonRequest::Resume { session_id }) => {
                    if self.session_router.is_local_session(&session_id) {
                        match self.handle_resume(&session_id).await {
                            Ok(()) => DaemonResponse::ResumeOk,
                            Err(e) => daemon_error(e.to_string(),),
                        }
                    } else if self.session_router.find_legacy_peer_for_session(&session_id).is_some() {
                        daemon_error("Session resume is not supported for peer sessions".to_string(),)
                    } else {
                        daemon_session_not_found(&session_id, "daemon_resume")
                    }
                }
                Ok(DaemonRequest::ListSessions) => {
                    let sessions = self.session_router.list_sessions().await;
                    DaemonResponse::ListSessionsOk {
                        epoch: self.epoch,
                        sessions,
                    }
                }
                Ok(DaemonRequest::DescribeLiveSessions) => {
                    let service = Arc::clone(&self.terminal_service);
                    match crate::ipc::run_blocking(move || {
                        Ok::<_, crate::ipc::IpcError>(service.describe_live_sessions())
                    })
                    .await
                    {
                        Ok(sessions) => DaemonResponse::DescribeLiveSessionsOk { epoch: self.epoch, sessions },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::Attach {
                    session_id,
                    after_sequence,
                }) => {
                    let agent_subscription = self.agent_states.subscribe(&session_id);
                    if let Err(error) = self.validate_session_ssh_target(&session_id).await {
                        daemon_error(error.to_string())
                    } else if self.session_router.is_local_session(&session_id) {
                        let is_remote = self.terminal_service.remote().contains(&session_id);
                        match self
                            .terminal_service
                            .attach_remote_with_sequence_ranges(&session_id, if is_remote { None } else { after_sequence })
                        {
                            Ok((mut attachment, history_ranges, remote_generation)) => {
                                if is_remote {
                                    attachment.snapshot.gap = Some(crate::terminal::output_hub::ReplayGap {
                                        requested_after_sequence: after_sequence.unwrap_or(0),
                                        available_from_sequence: attachment.snapshot.history_start_sequence.unwrap_or(1),
                                    });
                                }
                                let (pty_cols, pty_rows) = self
                                    .terminal_service
                                    .get_session(&session_id)
                                    .map(|s| s.get_size())
                                    .or_else(|| self.terminal_service.remote().details(&session_id).map(|d| (d.descriptor.cols, d.descriptor.rows)))
                                    .map(|(c, r)| (Some(c), Some(r)))
                                    .unwrap_or((None, None));
                                let hub = Arc::clone(self.terminal_service.output_hub());
                                let history_flat = Bytes::from(std::mem::take(&mut attachment.snapshot.history));
                                let history_segments = history_ranges
                                    .iter()
                                    .map(|r| HistorySegmentWire {
                                        cols: r.cols,
                                        rows: r.rows,
                                        bytes: history_flat.slice(r.start..r.end),
                                    })
                                    .collect();
                                let resp = DaemonResponse::AttachOk {
                                    epoch: self.epoch,
                                    session_id: session_id.clone(),
                                    start_sequence: attachment.snapshot.history_start_sequence,
                                    end_sequence: attachment.snapshot.history_end_sequence,
                                    gap: attachment.snapshot.gap,
                                    history: history_flat,
                                    pty_cols,
                                    pty_rows,
                                    history_segments,
                                    remote_generation,
                                };
                                let mut resp_json = serde_json::to_string(&resp).unwrap();
                                resp_json.push('\n');
                                let _ = write_half.write_all(resp_json.as_bytes()).await;
                                let _ = write_half.flush().await;

                                self.pump_session_stream(
                                    session_id,
                                    attachment.receiver,
                                    hub,
                                    write_half,
                                    Some(agent_subscription),
                                    Some(&mut reader),
                                )
                                .await;
                                return;
                            }
                            Err(crate::terminal::PtyError::SessionNotFound(_)) => daemon_session_not_found(&session_id, "daemon_attach"),
                            Err(e) => daemon_error(e),
                        }
                    } else if let Some(peer) = self.session_router.find_legacy_peer_for_session(&session_id) {
                        if let Err(e) = peer.attach_and_stream(
                            &session_id,
                            after_sequence,
                            &mut write_half,
                            &mut reader,
                            agent_subscription,
                            Arc::clone(&self.agent_states),
                        ).await {
                            daemon_error(e)
                        } else {
                            return;
                        }
                    } else {
                        daemon_session_not_found(&session_id, "daemon_attach")
                    }
                }
                Ok(DaemonRequest::SaveSession { session }) => {
                    let res = crate::ipc::run_blocking(move || save_session_to_path(&get_default_session_path(), &session)).await;
                    match res {
                        Ok(()) => DaemonResponse::SaveSessionOk,
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::LoadSession) => {
                    match crate::ipc::run_blocking(move || load_session_from_path(&get_default_session_path())).await {
                        Ok(session) => DaemonResponse::LoadSessionOk { session },
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::ClearSession) => {
                    match crate::ipc::run_blocking(move || clear_session_from_path(&get_default_session_path())).await {
                        Ok(()) => DaemonResponse::ClearSessionOk,
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::RemoteGetStatus) => {
                    let config = self.remote_state.config.read().clone();
                    let is_running = *self.remote_state.is_running.read();
                    let bound_address = self.remote_state.bound_address.read().clone();
                    let identity = crate::remote::auth::canonical_identity_dir()
                        .ok()
                        .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok());
                    let machine_id = identity.map(|id| id.machine_id);
                    let (relay_connected, control_channel_connected) = {
                        let relay_client = self.remote_state.relay_client.read();
                        crate::remote::relay_client::relay_connection_flags(relay_client.as_ref())
                    };
                    let (gate_status, gate_reason) = {
                        let handle_guard = self.remote_server_handle.lock();
                        if let Some(h) = handle_guard.as_ref() {
                            let gs = h.gate_status();
                            let reason = match &gs {
                                crate::remote::server::DirectGatewayGateStatus::InsecureLanGated { reason, .. } => Some(reason.clone()),
                                _ => None,
                            };
                            (Some(gs), reason)
                        } else {
                            (None, None)
                        }
                    };
                    DaemonResponse::RemoteStatusOk {
                        status: DaemonRemoteStatus {
                            mode: config.mode,
                            port: config.port,
                            allow_control: config.allow_control,
                            is_running,
                            bound_address,
                            relay_url: config.relay_url,
                            machine_id,
                            relay_connected,
                            control_channel_connected,
                            gate_status,
                            gate_reason,
                            insecure_modes: crate::ipc::run_blocking(|| {
                                Ok(crate::remote::security_settings::active_insecure_modes())
                            })
                            .await
                            .unwrap_or_default(),
                        },
                    }
                }
                Ok(DaemonRequest::RemoteConfigure { config }) => {
                    match self.handle_remote_configure(config).await {
                        Ok(()) => DaemonResponse::RemoteConfigureOk,
                        Err(e) => daemon_error(e),
                    }
                }
                Ok(DaemonRequest::PairedTerminalReattach { descriptor }) => {
                    let result = self.terminal_service.paired().get_or_rearm(
                        descriptor, self.terminal_service.output_hub().clone(), self.paired_hosts.clone(),
                    );
                    match result {
                        Ok((session_id, generation)) => DaemonResponse::PairedTerminalReattachOk { session_id, generation },
                        Err(error) => DaemonResponse::PairedHostOperationError { error },
                    }
                }
                Ok(DaemonRequest::PairedTerminalDetach { session_id }) => match self.terminal_service.paired().detach(&session_id).await {
                    Ok(()) => DaemonResponse::CloseOk,
                    Err(message) => daemon_error(message),
                },
                Ok(DaemonRequest::RetryPairedSession { backend_session_id, daemon_epoch, incarnation }) => {
                    match self.terminal_service.paired().retry_owned(&backend_session_id, &daemon_epoch, &incarnation).await {
                        Ok(result) => DaemonResponse::RetryPairedSessionResult {
                            admitted: result.admitted,
                            owner_unavailable: result.owner_unavailable,
                            stale_tuple: result.stale_tuple,
                            blocked: result.blocked,
                            affirmative_exit: result.affirmative_exit,
                        },
                        Err(code) => daemon_error(code),
                    }
                }
                Ok(DaemonRequest::PairedRecoveryOutcome { backend_session_id }) => {
                    DaemonResponse::PairedRecoveryOutcomeResult {
                        outcome: self.terminal_service.paired().recovery_outcome(&backend_session_id),
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedDropLoopbackTransport { backend_session_id }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match self.terminal_service.paired().qa_drop_loopback_transport(&backend_session_id).await {
                            Ok(()) => DaemonResponse::CloseOk,
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedCancelPendingDial { backend_session_id }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match self.terminal_service.paired().qa_cancel_pending_dial(&backend_session_id).await {
                            Ok(()) => DaemonResponse::CloseOk,
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedRouteReceipt { backend_session_id }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match self.terminal_service.paired().qa_route_receipt(&backend_session_id).await {
                            Ok(receipt) => DaemonResponse::QaPairedRouteReceiptOk { receipt },
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedColdDescriptor { backend_session_id }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match self.terminal_service.paired().qa_cold_descriptor(&backend_session_id).await {
                            Ok(descriptor) => DaemonResponse::PairedTerminalDescriptorOk { descriptor: Some(descriptor) },
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedMutation { backend_session_id, daemon_epoch, incarnation, generation, operation, intent_id, data }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        async {
                            let paired = self.terminal_service.paired();
                            let pending = paired.qa_mutation_owned(&backend_session_id, &daemon_epoch,
                                &incarnation, generation, &operation, intent_id.clone(), data);
                            match pending {
                                Ok(pending) => match pending.await {
                                    Ok(()) => DaemonResponse::QaPairedMutationOk { intent_id },
                                    Err(failure) => DaemonResponse::RemoteSessionError { failure },
                                },
                                Err(error) => daemon_error(error),
                            }
                        }.await
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedDialAudit { backend_session_id }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match self.terminal_service.paired().qa_dial_audit(&backend_session_id).await {
                            Ok(audit) => DaemonResponse::QaPairedDialAuditOk { audit },
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedOwnerAudit { backend_session_id }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else if let Some((outcome, incarnation, generation, descriptor)) = self.terminal_service.paired().qa_owner_audit(&backend_session_id) {
                        DaemonResponse::QaPairedOwnerAuditOk {
                            outcome,
                            daemon_epoch: descriptor.target.daemon_epoch.0.to_string(),
                            incarnation,
                            generation: generation.map(|epoch| epoch.0),
                            descriptor: Some(descriptor),
                        }
                    } else {
                        DaemonResponse::Error { message: "Paired owner unavailable".into(), code: Some("OWNER_UNAVAILABLE".into()), details: None }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedOwnerWaitState { backend_session_id, state }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match self.terminal_service.paired().qa_wait_owner_state(&backend_session_id, &state, std::time::Duration::from_secs(10)).await {
                            Ok((outcome, generation)) => DaemonResponse::QaPairedOwnerWaitStateOk { outcome, generation: generation.map(|epoch| epoch.0) },
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedOwnerRetry { backend_session_id, daemon_epoch, incarnation }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match self.terminal_service.paired().retry_owned(&backend_session_id, &daemon_epoch, &incarnation).await {
                            Ok(result) => DaemonResponse::QaPairedOwnerRetryOk { result },
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedHostDeliveryArmFence { target, device_id, generation, expected_write_multiplicity, expected_signal_multiplicity }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        async {
                        let Some(services) = self.remote_state.machine_services.as_ref() else {
                            return daemon_error("MACHINE_SERVICES_UNAVAILABLE");
                        };
                        if let Err(err) = services.sessions.validate_machine_target(&target).await {
                            return daemon_error(err);
                        }
                        let controllers = services.sessions.machine_controllers.lock().await;
                        let Some(c) = controllers.get(&target.session_id) else {
                            return daemon_error("CONTROLLER_NOT_FOUND");
                        };
                        if c.device != device_id {
                            return daemon_error("CONTROLLER_DEVICE_MISMATCH");
                        }
                        if c.generation != generation.0 {
                            return daemon_error("STALE_CONTROLLER_GENERATION");
                        }
                        if c.disconnected.lock().is_some() {
                            return daemon_error("CONTROLLER_DISCONNECTED");
                        }
                        let res = crate::remote::server::machine_delivery_observer::arm_fence(
                            &target,
                            generation,
                            expected_write_multiplicity,
                            expected_signal_multiplicity,
                        );
                        drop(controllers);
                        match res {
                            Ok(fence_id) => DaemonResponse::QaPairedHostDeliveryArmFenceOk { fence_id },
                            Err(code) => daemon_error(code),
                        }
                        }.await
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedHostDeliverySealFence { fence_id, target }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match crate::remote::server::machine_delivery_observer::seal_fence(&fence_id, &target) {
                            Ok(fence) => match serde_json::to_value(fence) {
                                Ok(fence) => DaemonResponse::QaPairedHostDeliverySealFenceOk { fence },
                                Err(error) => daemon_error(error),
                            },
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                #[cfg(feature = "local-split-qa")]
                Ok(DaemonRequest::QaPairedHostDeliveryAudit { target, fence_id }) => {
                    if !active_transport_qa_profile() {
                        daemon_error("QA_ISOLATION_REQUIRED")
                    } else {
                        match crate::remote::server::machine_delivery_observer::audit(&target, &fence_id) {
                            Ok(audit) => DaemonResponse::QaPairedHostDeliveryAuditOk { audit },
                            Err(code) => daemon_error(code),
                        }
                    }
                }
                Ok(DaemonRequest::PairedTerminalDescriptor { session_id }) => {
                    let descriptor = self.terminal_service.paired().descriptor(&session_id);
                    DaemonResponse::PairedTerminalDescriptorOk { descriptor }
                }
                Ok(DaemonRequest::PairedHostOperation { request }) => match crate::paired_host::client::MachineClient::new().execute(&self.paired_hosts, request).await {
                    Ok(response) => DaemonResponse::PairedHostOperationOk { response },
                    Err(error) => DaemonResponse::PairedHostOperationError { error },
                },
                Ok(DaemonRequest::PairedHostRead { request }) => match self.paired_hosts.read(request).await {
                    Ok(host) => DaemonResponse::PairedHostReadOk { host },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostList) => match self.paired_hosts.list().await {
                    Ok(hosts) => DaemonResponse::PairedHostListOk { hosts },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostReachabilityList) => match self.paired_hosts.reachability_list().await {
                    Ok(entries) => DaemonResponse::PairedHostReachabilityListOk { entries },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostRouteList) => match self.paired_hosts.list().await {
                    Ok(hosts) => DaemonResponse::PairedHostRouteListOk {
                        entries: hosts
                            .into_iter()
                            .map(|host| crate::paired_host::route_view::HostRoute::current(
                                &crate::paired_host::direct_route::GLOBAL_DIRECT_ROUTES,
                                host.host_id,
                            ))
                            .collect(),
                    },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostPair { request }) => match self.paired_hosts.pair(request).await {
                    Ok(host) => DaemonResponse::PairedHostPairOk { host },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostMigrateLegacy { request }) => match self.paired_hosts.migrate_legacy(request).await {
                    Ok(receipt) => DaemonResponse::PairedHostMigrateLegacyOk { receipt },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostRedeemGrant { request }) => match self.paired_hosts.redeem_grant(request).await {
                    Ok(host) => DaemonResponse::PairedHostPairOk { host },
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostClientAttachKey) => match crate::ipc::run_blocking(|| {
                    Ok(crate::paired_host::secure_http::client_attach_public_key())
                }).await {
                    Ok(Ok(public_key)) => DaemonResponse::PairedHostClientAttachKeyOk { public_key },
                    Ok(Err(error)) => DaemonResponse::PairedHostError {
                        error: crate::paired_host::service::ServiceError::new(error.code, "client attach identity unavailable"),
                    },
                    Err(_) => DaemonResponse::PairedHostError {
                        error: crate::paired_host::service::ServiceError::unavailable(),
                    },
                },
                Ok(DaemonRequest::PairedHostForget { host_id, expected_generation }) => match self.paired_hosts.forget(host_id, expected_generation).await {
                    Ok(()) => DaemonResponse::PairedHostForgetOk,
                    Err(error) => DaemonResponse::PairedHostError { error },
                },
                Ok(DaemonRequest::PairedHostRevoke { host_id, generation }) => {
                    self.paired_hosts.revoke_on_auth_failure(host_id, generation).await;
                    DaemonResponse::PairedHostForgetOk
                }
                Ok(DaemonRequest::GetCapabilities) => {
                    let mut capabilities = vec!["machinePairingV1".into(), "sshPasswordV1".into(), "dagStreamingV1".into()];
                    if self.paired_hosts.available().await {
                        capabilities.push("pairedHostInventoryV1".into());
                        capabilities.push(crate::paired_host::reachability::PAIRED_HOST_REACHABILITY_CAPABILITY.into());
                        capabilities.push(crate::paired_host::route_view::PAIRED_HOST_ROUTE_CAPABILITY.into());
                        capabilities.push("pairedHostE2EV1".into());
                    }
                    capabilities.push(crate::paired_host::file_read::PAIRED_FILE_READ_PROXY_CAPABILITY.into());
                    #[cfg(unix)]
                    capabilities.push("sessionOwnershipTransferV1".into());
                    capabilities.push(crate::agent_control::AGENT_CONTROL_CAPABILITY.into());
                    capabilities.push(crate::agent_control::AGENT_CONTROL_REMOTE_CAPABILITY.into());
                    capabilities.push(crate::hub_runtime::SUPERVISOR_GOAL_CAPABILITY.into());
                    DaemonResponse::CapabilitiesOk { capabilities }
                },
                Ok(DaemonRequest::GetAgentIntegration) => DaemonResponse::AgentIntegrationOk {
                    enabled: self.agent_integration.enabled(),
                },
                Ok(DaemonRequest::SupervisorSnapshot) => DaemonResponse::SupervisorSnapshotOk {
                    snapshot: crate::hub_runtime::supervisor_snapshot(self.remote_state()).await,
                },
                Ok(DaemonRequest::AgentControlSettingsGet) => DaemonResponse::AgentControlSettingsOk {
                    settings: self.remote_state.agent_control_settings(),
                },
                Ok(DaemonRequest::AgentControlSettingsSet { enabled, excluded_device_ids }) => {
                    let state = Arc::clone(&self.remote_state);
                    match tokio::task::spawn_blocking(move || state.set_agent_control(enabled, excluded_device_ids)).await {
                        Ok(Ok(settings)) => DaemonResponse::AgentControlSettingsOk { settings },
                        Ok(Err(error)) => DaemonResponse::Error {
                            message: format!("Failed to save the agent control setting: {error}"),
                            code: Some("IO_ERROR".into()),
                            details: None,
                        },
                        Err(error) => DaemonResponse::Error {
                            message: format!("Agent control setting task failed: {error}"),
                            code: Some("INTERNAL_ERROR".into()),
                            details: None,
                        },
                    }
                }
                Ok(DaemonRequest::SupervisorSetPaused { lane_key, paused }) => {
                    match crate::hub_runtime::set_lane_paused(self.remote_state(), &lane_key, paused, "desktop").await {
                        Ok(_) => DaemonResponse::SupervisorSetPausedOk { paused },
                        Err(error) => DaemonResponse::Error {
                            message: error.to_string(),
                            code: Some("SUPERVISOR_UNAVAILABLE".into()),
                            details: None,
                        },
                    }
                }
                Ok(DaemonRequest::SupervisorGoal { request }) => {
                    match crate::hub_runtime::supervisor_goal(self.remote_state(), request).await {
                        Ok(snapshot) => DaemonResponse::SupervisorGoalOk { snapshot },
                        Err(error) => DaemonResponse::Error {
                            message: error.to_string(),
                            code: Some(error.code().into()),
                            details: None,
                        },
                    }
                }
                Ok(DaemonRequest::SetAgentIntegration { enabled }) => {
                    let setting = Arc::clone(&self.agent_integration);
                    let failure = match tokio::task::spawn_blocking(move || setting.set(enabled)).await {
                        Ok(Ok(())) => None,
                        Ok(Err(error)) => {
                            Some(format!("Failed to save the agent integration setting: {error}"))
                        }
                        Err(error) => Some(format!("Agent integration setting task failed: {error}")),
                    };
                    match failure {
                        None => DaemonResponse::AgentIntegrationOk { enabled },
                        Some(message) => DaemonResponse::Error {
                            message,
                            code: Some(crate::daemon::protocol::AGENT_INTEGRATION_ERROR_CODE.into()),
                            details: None,
                        },
                    }
                }
                Ok(DaemonRequest::RemoteAllocateAttachSession) => {
                    let client = self.remote_state.relay_client.read().clone();
                    let relay_origin = self
                        .remote_state
                        .config
                        .read()
                        .relay_url
                        .clone()
                        .unwrap_or_default();
                    match client {
                        None => DaemonResponse::Error {
                            message: "relay mode is not running".into(),
                            code: Some("RELAY_OFF".into()),
                            details: None,
                        },
                        Some(client) => match client.allocate_opaque_session().await {
                        Ok(session_id) => {
                            let identity = crate::remote::auth::canonical_identity_dir()
                                .ok()
                                .and_then(|dir| {
                                    crate::remote::auth::load_or_generate_machine_identity(&dir).ok()
                                });
                            let attach = crate::remote::attach_identity::
                                load_or_generate_canonical_attach_identity();
                            match (identity, attach) {
                                (Some(identity), Ok(attach)) => {
                                    DaemonResponse::RemoteAttachSessionOk {
                                        session_id,
                                        machine_id: identity.machine_id,
                                        machine_attach_public_key: attach.public_key,
                                        enrollment_epoch: crate::account::enroll_client::
                                            load_enrollment_record()
                                            .map(|record| record.enrollment_epoch)
                                            .unwrap_or_default(),
                                        relay_origin,
                                    }
                                }
                                (None, _) => DaemonResponse::Error {
                                    message: "machine identity unavailable".into(),
                                    code: Some("IDENTITY_UNAVAILABLE".into()),
                                    details: None,
                                },
                                (_, Err(error)) => DaemonResponse::Error {
                                    message: error,
                                    code: Some("ATTACH_IDENTITY_UNAVAILABLE".into()),
                                    details: None,
                                },
                            }
                        }
                            Err(error) => DaemonResponse::Error {
                                message: error.to_string(),
                                code: Some("RELAY_ALLOCATE_FAILED".into()),
                                details: None,
                            },
                        },
                    }
                }
                Ok(request @ (DaemonRequest::RemoteCreatePairingCode { .. }
                    | DaemonRequest::RemoteCreateMachinePairingCode)) => {
                    let (permission, scope) = match request {
                        DaemonRequest::RemoteCreatePairingCode { permission } =>
                            (permission, crate::remote::auth::DeviceAccessScope::Mirror),
                        DaemonRequest::RemoteCreateMachinePairingCode =>
                            (Some(DevicePermission::Control), crate::remote::auth::DeviceAccessScope::Machine),
                        _ => unreachable!("pairing request pattern"),
                    };
                    let perm = permission.unwrap_or(DevicePermission::Control);
                    let mut auto_configured = false;
                    let mut config_error = None;
                    if self.remote_state.relay_pairing.read().is_none() {
                        let mode = self.remote_state.config.read().mode;
                        if mode == RemoteNetworkMode::Off {
                            let relay_url = std::env::var("FERRYX_RELAY_URL")
                                .ok()
                                .filter(|s| !s.trim().is_empty())
                                .or_else(|| self.remote_state.config.read().relay_url.clone())
                                .or_else(|| Some(crate::remote::relay_server::DEFAULT_RELAY_URL.to_string()));
                            if let Some(relay_url) = relay_url {
                                let mut config = self.remote_state.config.read().clone();
                                config.mode = RemoteNetworkMode::Relay;
                                config.relay_url = Some(relay_url);
                                if let Err(e) = self.handle_remote_configure(config).await {
                                    config_error = Some(e);
                                } else {
                                    auto_configured = true;
                                }
                            }
                        }
                    }

                    if let Some(e) = config_error {
                        daemon_error(format!("Relay is unreachable or registration failed: {e}"),)
                    } else {
                        let is_relay_mode = auto_configured
                            || self.remote_state.config.read().mode == RemoteNetworkMode::Relay;

                        // In relay mode a code is only redeemable remotely if its PIN was
                        // registered with the relay, so go through the gateway's single
                        // pairing coordinator rather than minting a local-only code.
                        let coordinator = self
                            .remote_state
                            .relay_pairing
                            .read()
                            .as_ref()
                            .map(|published| published.coordinator.clone());
                        match coordinator {
                            Some(coordinator) => {
                                match coordinator
                                    .generate_scoped_pairing(
                                        std::time::Duration::from_secs(if scope == crate::remote::auth::DeviceAccessScope::Machine { 600 } else { 60 }),
                                        perm,
                                        scope,
                                    )
                                    .await
                                {
                                    Ok(info) => {
                                        let effective_relay_url = self
                                            .remote_state
                                            .config
                                            .read()
                                            .relay_url
                                            .clone()
                                            .or_else(|| {
                                                std::env::var("FERRYX_RELAY_URL")
                                                    .ok()
                                                    .filter(|s| !s.trim().is_empty())
                                            })
                                            .or_else(|| {
                                                Some(crate::remote::relay_server::DEFAULT_RELAY_URL.to_string())
                                            });
                                        DaemonResponse::RemotePairingCodeOk {
                                            code: info.pin,
                                            pairing_token: Some(info.pairing_token),
                                            machine_id: Some(info.machine_id),
                                            relay_url: effective_relay_url,
                                        }
                                    }
                                    Err(message) => {
                                        let message = if message.starts_with("Relay is unreachable or registration failed:") {
                                            message
                                        } else {
                                            format!("Relay is unreachable or registration failed: {message}")
                                        };
                                        daemon_error(message)
                                    }
                                }
                            }
                            None if is_relay_mode => daemon_error("Relay is unreachable or registration failed: relay client is not connected".to_string(),),
                            None => {
                                match self.remote_state.auth_manager.create_scoped_pairing_code(perm, scope) {
                                    Ok(code) => {
                                        let machine_id = if scope == crate::remote::auth::DeviceAccessScope::Machine {
                                            crate::remote::auth::canonical_identity_dir()
                                                .ok()
                                                .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok())
                                                .map(|id| id.machine_id)
                                        } else {
                                            None
                                        };
                                        DaemonResponse::RemotePairingCodeOk {
                                            code,
                                            pairing_token: None,
                                            machine_id,
                                            relay_url: None,
                                        }
                                    }
                                    Err(error) => daemon_error(error.to_string()),
                                }
                            }
                        }
                    }
                }
                Ok(DaemonRequest::RemoteListDevices) => {
                    let devices = self.remote_state.auth_manager.list_devices();
                    DaemonResponse::RemoteListDevicesOk { devices }
                }
                Ok(DaemonRequest::RemoteRevokeDevice { device_id }) => {
                    match self.remote_state.auth_manager.revoke_device(&device_id) {
                        Ok(true) => DaemonResponse::RemoteRevokeDeviceOk,
                        Ok(false) => daemon_error(format!("Device '{device_id}' not found"),),
                        Err(err) => daemon_error(format!("Failed to persist revocation: {err}"),),
                    }
                }
                Ok(DaemonRequest::RemoteSetActiveSelection { selection, ssh_store_path }) => {
                    if let Some(path) = ssh_store_path {
                        *self.remote_state.ssh_store_path.write() = Some(path);
                    }
                    self.remote_state.set_active_selection_opt(selection);
                    DaemonResponse::RemoteSetActiveSelectionOk
                }
                Ok(DaemonRequest::RemoteGetActiveSelection) => {
                    let selection = self.remote_state.active_selection.read().clone();
                    DaemonResponse::RemoteGetActiveSelectionOk { selection }
                }
                Ok(DaemonRequest::SubscribeRemoteEvents) => {
                    let mut events = self.remote_event_tx.subscribe();
                    let mut resp_json =
                        serde_json::to_string(&DaemonResponse::SubscribeRemoteEventsOk).unwrap();
                    resp_json.push('\n');
                    if write_half.write_all(resp_json.as_bytes()).await.is_err()
                        || write_half.flush().await.is_err()
                    {
                        return;
                    }
                    loop {
                        match events.recv().await {
                            Ok(event) => {
                                let Ok(mut line) = serde_json::to_string(&event) else {
                                    continue;
                                };
                                line.push('\n');
                                if write_half.write_all(line.as_bytes()).await.is_err()
                                    || write_half.flush().await.is_err()
                                {
                                    return;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
                Ok(DaemonRequest::UpgradeBinary { new_binary_path }) => {
                    self.handle_upgrade_binary(new_binary_path).await
                }
                Ok(DaemonRequest::PrepareHandover) => {
                    #[cfg(unix)]
                    {
                        match self.prepare_handover_shared().await {
                            Ok((legacy_path, active_sessions, listener)) => {
                                let legacy_path_str = legacy_path.to_string_lossy().into_owned();
                                Arc::clone(&self).spawn_legacy_listener(listener);
                                DaemonResponse::PrepareHandoverOk {
                                    legacy_socket_path: legacy_path_str,
                                    active_sessions,
                                }
                            }
                            Err(e) => handover_failure_response(None, e),
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        DaemonResponse::HandoverRejected {
                            reason: "Handover is only supported on Unix platforms".to_string(),
                        }
                    }
                }
                Ok(DaemonRequest::TransferSessions { handover_socket_path }) => {
                    #[cfg(unix)]
                    {
                        crate::daemon::handover_request::note_transfer_started();
                        if let Err(e) = self.persist_remote_sessions_at(self.remote_sessions_path.clone()).await {
                            daemon_error(format!("Failed to persist remote sessions before handover: {e}"))
                        } else {
                            let path = std::path::PathBuf::from(handover_socket_path);
                            match crate::daemon::handover_socket::connect_handover_socket(&path) {
                                Ok((stream, _creds)) => {
                                    let sessions = self.terminal_service.pty_manager().list_sessions();
                                    let requested = sessions.len();
                                    let mut count = 0;
                                    let mut failures: Vec<String> = Vec::new();
                                    let transfer_id = uuid::Uuid::new_v4().to_string();
                                    let mut seq = 1;
                                    for session_id in sessions {
                                        // A session dropped here dies when this predecessor retires: it
                                        // has already promised the successor a handover, and nothing
                                        // downstream can recover a session that was never sent. Name
                                        // every casualty instead of silently skipping it.
                                        match self.terminal_service.pty_manager().export_session(&session_id) {
                                            Ok(export) => {
                                                self.handover_manager.note_transferred_local(&session_id);
                                                // The successor can only take ownership of a session
                                                // with the record that proves who owns it, so the
                                                // owner's own record travels with the export. A
                                                // session exported without one is refused by the
                                                // successor, which keeps it here instead of losing it.
                                                let owner = {
                                                    let metadata = self.session_metadata.read();
                                                    metadata.get(&session_id)
                                                        .and_then(|meta| serde_json::to_value(meta).ok())
                                                };
                                                if owner.is_none() {
                                                    tracing::error!(
                                                        session_id = %session_id,
                                                        "No authoritative owner record for this session; the successor will refuse it"
                                                    );
                                                }
                                                match crate::daemon::handover_socket::send_session(&stream, &transfer_id, seq, export, owner) {
                                                    Ok(()) => {
                                                        count += 1;
                                                        seq += 1;
                                                        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                                        crate::daemon::qa_producers::note_predecessor_export_off_runtime(
                                                            crate::ipc::qa_barrier::active_channel().as_ref(),
                                                            &session_id,
                                                        )
                                                        .await;
                                                    }
                                                    Err(error) => {
                                                        tracing::error!(
                                                            session_id = %session_id,
                                                            %error,
                                                            "Failed to send a session to the successor; it will not survive this handover"
                                                        );
                                                        failures.push(format!("{session_id}: send failed: {error}"));
                                                    }
                                                }
                                            }
                                            Err(error) => {
                                                tracing::error!(
                                                    session_id = %session_id,
                                                    %error,
                                                    "Failed to export a session for handover; it will not survive this handover"
                                                );
                                                failures.push(format!("{session_id}: export failed: {error}"));
                                            }
                                        }
                                    }
                                    // A TRANSFER_DONE that never reached the candidate leaves it
                                    // without a complete transfer: record it, skip the authority
                                    // offer on the broken stream, and answer an error below so the
                                    // candidate aborts and this predecessor keeps serving.
                                    let done = send_transfer_done_recorded(&stream, &transfer_id, seq, &mut failures);
                                    if done {
                                        // Authority transfer (todo 8): the candidate keeps this stream
                                        // open and asks for the public listeners and lock fds once it
                                        // has adopted the sessions, before it sends CommitHandover. The
                                        // answer carries duplicates only; this daemon keeps serving on
                                        // its own copies until its commit decision is written. A
                                        // candidate that closes the stream without asking (a pre-plan
                                        // v5 successor) gets nothing.
                                        match self.handover_manager.authority_sources() {
                                            Ok(sources) => {
                                                #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                                let transfer_fault = crate::daemon::qa_producers::armed_transfer_fault();
                                                tokio::task::spawn_blocking(move || {
                                                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                                    let mut sources = sources;
                                                    // QA only: todo 8's authority-transfer failure
                                                    // variants, armed by the runner for this handover.
                                                    // The stray listener lives until the send is done.
                                                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                                    let stray_path = get_runtime_dir().join(format!("qa-stray-{}.sock", std::process::id()));
                                                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                                    let _stray = match transfer_fault {
                                                        Some(fault) => match crate::daemon::qa_producers::apply_transfer_fault(
                                                            fault,
                                                            &mut sources,
                                                            &get_lock_path(),
                                                            &stray_path,
                                                        ) {
                                                            Ok(stray) => {
                                                                tracing::warn!(?fault, "QA transfer fault applied to the authority");
                                                                stray
                                                            }
                                                            Err(error) => {
                                                                tracing::warn!(%error, ?fault, "QA transfer fault could not be applied; authority not served");
                                                                return;
                                                            }
                                                        },
                                                        None => None,
                                                    };
                                                    // The authority request is read with the request's
                                                    // own TTL as its bound; without one the worker could
                                                    // block forever, so a failure ends this request.
                                                    if let Err(error) = stream.set_read_timeout(Some(Duration::from_millis(
                                                        crate::daemon::handover_request::REQUEST_TTL_MS,
                                                    ))) {
                                                        tracing::warn!(%error, "Authority request not served: could not bound its read with a timeout");
                                                        return;
                                                    }
                                                    match crate::daemon::handover_socket::serve_authority_request(&stream, sources) {
                                                        Ok(()) => tracing::info!("Sent authority descriptors to the handover candidate"),
                                                        Err(crate::daemon::handover_socket::HandoverSocketError::Wire(
                                                            crate::daemon::handover_wire::HandoverWireError::Closed,
                                                        )) => {}
                                                        Err(error) => tracing::warn!(%error, "Authority request was not served"),
                                                    }
                                                    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
                                                    if _stray.is_some() {
                                                        let _ = std::fs::remove_file(&stray_path);
                                                    }
                                                });
                                            }
                                            Err(error) => tracing::warn!(%error, "No authority descriptors to offer the handover candidate"),
                                        }
                                    }
                                    // The gap between what this predecessor was asked for and what it
                                    // could actually export is the shape of a lossy handover.
                                    if !failures.is_empty() {
                                        tracing::error!(
                                            requested,
                                            transferred = count,
                                            lost = failures.len(),
                                            casualties = %failures.join("; "),
                                            "Handover will lose sessions: they could not be exported or sent"
                                        );
                                    }
                                    transfer_sessions_reply(done, count, requested, &failures)
                                }
                                Err(e) => daemon_error(format!("Failed to connect to handover socket: {e}")),
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        DaemonResponse::HandoverRejected {
                            reason: "Session transfer unsupported on non-Unix".to_string(),
                        }
                    }
                }
                Ok(DaemonRequest::CommitHandover { .. }) => {
                    let manager = Arc::clone(&self.handover_manager);
                    let service = Arc::clone(&self.terminal_service);
                    // Injected reply loss (tests / QA): before the decision is recorded the
                    // predecessor keeps serving; after it, the decision alone settles ownership.
                    let fault = manager.commit_fault();
                    if fault == Some(super::handover::CommitFault::DropReplyBeforeRecord) {
                        tracing::warn!("Injected fault: CommitHandoverOk dropped before the decision");
                        break;
                    }
                    match crate::ipc::run_blocking(move || {
                        manager.commit_handover(&service).map_err(crate::ipc::IpcError::internal)
                    }).await {
                        Ok(()) if fault == Some(super::handover::CommitFault::DropReplyAfterRecord) => {
                            tracing::warn!("Injected fault: CommitHandoverOk dropped after the decision");
                            self.release_transferred_locals().await;
                            break;
                        }
                        Ok(()) => {
                            self.release_transferred_locals().await;
                            #[cfg(all(unix, feature = "local-split-qa", feature = "native-terminal"))]
                            crate::daemon::qa_producers::note_authority_stage("predecessor-committed", &self.handover_manager).await;
                            DaemonResponse::CommitHandoverOk
                        }
                        Err(e) => handover_failure_response(None, e.to_string()),
                    }
                }
                Ok(DaemonRequest::AbortHandover) => {
                    // `abort_handover` writes the decision transaction with create_new + fsync +
                    // rename + parent-directory fsync, so it is offloaded exactly like the commit
                    // path above instead of stalling a worker of the runtime that serves this loop.
                    let manager = Arc::clone(&self.handover_manager);
                    match crate::ipc::run_blocking(move || {
                        manager.abort_handover().map_err(crate::ipc::IpcError::internal)
                    }).await {
                        Ok(()) => {
                            self.handover_manager.clear_transferred_locals();
                            #[cfg(all(unix, feature = "local-split-qa", feature = "native-terminal"))]
                            crate::daemon::qa_producers::note_authority_stage("predecessor-aborted", &self.handover_manager).await;
                            let resumed = self
                                .terminal_service
                                .pty_manager()
                                .resume_paused_readers();
                            tracing::warn!(
                                resumed_count = resumed,
                                "Predecessor resumed serving {resumed} exported session(s) after AbortHandover"
                            );
                            DaemonResponse::AbortHandoverOk
                        }
                        Err(e) => daemon_error(e.to_string()),
                    }
                }
                Ok(DaemonRequest::UploadClipboardImage { file_name, data }) => {
                    match crate::clipboard_image::save_paste_file(&file_name, &data) {
                        Ok(path) => DaemonResponse::UploadClipboardImageOk {
                            remote_path: path.to_string_lossy().into_owned(),
                            byte_length: data.len(),
                        },
                        Err(e) => daemon_error(e.message),
                    }
                }
                Ok(DaemonRequest::SubscribeDag { workspace_id, project_path, paired }) => {
                    let mut resp_json = serde_json::to_string(&DaemonResponse::SubscribeDagOk).unwrap();
                    resp_json.push('\n');
                    if write_half.write_all(resp_json.as_bytes()).await.is_err()
                        || write_half.flush().await.is_err()
                    {
                        return;
                    }
                    match paired {
                        // A paired workspace's journal lives on the remote machine.
                        // Scanning project_path here would read an unrelated local
                        // directory that happens to share the name, or nothing at all.
                        Some(binding) => {
                            self.serve_paired_dag_stream(&mut reader, &mut write_half, workspace_id, binding).await;
                        }
                        None => {
                            self.serve_dag_stream(&mut reader, &mut write_half, workspace_id, project_path).await;
                        }
                    }
                    return;
                }
                Ok(DaemonRequest::UnsubscribeDag { .. }) => {
                    DaemonResponse::UnsubscribeDagOk
                }
                Ok(DaemonRequest::Shutdown) => {
                    tracing::info!("Daemon shutdown requested over the control socket");
                    match self.persist_remote_sessions_at(self.remote_sessions_path.clone()).await {
                        Ok(()) => std::process::exit(0),
                        Err(message) => daemon_error(message),
                    }
                }
                Ok(DaemonRequest::AgentControl { request, host_id }) => {
                    match host_id {
                        None => match crate::remote::server::agent_control_service::handle(&self.remote_state, request).await {
                            Ok(result) => DaemonResponse::AgentControlOk { result },
                            Err(error) => crate::remote::server::agent_control_service::error_response(error),
                        },
                        // One execute, on the caller's request id. A lost answer stays outcome-unknown
                        // and is not issued again.
                        Some(host_id) => {
                            crate::remote::server::agent_control_service::forward_daemon(
                                &self.remote_state,
                                &self.paired_hosts,
                                host_id,
                                request,
                            )
                            .await
                        }
                    }
                }
                Err(e) => daemon_error(format!("Malformed request: {e}"),),
            };

            let mut resp_json = serde_json::to_string(&resp).unwrap();
            resp_json.push('\n');
            if write_half.write_all(resp_json.as_bytes()).await.is_err() {
                break;
            }
            let _ = write_half.flush().await;
            if matches!(resp, DaemonResponse::ProtocolMismatch { .. }) {
                break;
            }
            if should_check_retirement {
                self.handover_manager
                    .check_retirement_if_empty(&self.terminal_service);
            }
            #[cfg(test)]
            if is_close {
                if let Some(park) = self.close_park.get() {
                    park.0.notify_one();
                    park.1.notified().await;
                }
            }
            // Dropping the last guard re-checks the retirement predicate (handover.rs).
            drop(retirement_guard);
                }
            }
        }
    }

    /// The one prepare path of a predecessor: the PrepareHandover request and control
    /// RequestUpgrade both call it. Under local-split-qa it first arms the runner's handover faults
    /// for this handover and, once prepared, writes the predecessor-prepared receipt (todo 12 Q08).
    /// prepare_handover removes, binds and chmods the legacy socket, so it runs off the workers.
    #[cfg(unix)]
    pub(crate) async fn prepare_handover_shared(
        self: &Arc<Self>,
    ) -> Result<(PathBuf, Vec<String>, UnixListener), String> {
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        crate::daemon::qa_producers::arm_handover_faults(&self.handover_manager).await;
        let (manager, terminals) = (Arc::clone(&self.handover_manager), Arc::clone(&self.terminal_service));
        let prepared = crate::ipc::run_blocking(move || {
            manager.prepare_handover(&terminals).map_err(crate::ipc::IpcError::internal)
        })
        .await
        .map_err(|error| error.to_string())?;
        #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
        crate::daemon::qa_producers::note_authority_stage("predecessor-prepared", &self.handover_manager).await;
        Ok(prepared)
    }

    #[cfg(unix)]
    pub fn spawn_legacy_listener(self: Arc<Self>, listener: UnixListener) {
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&self);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
    }

    /// Streams a paired workspace's DAG from the authenticated remote host.
    ///
    /// The desktop daemon owns no copy of that journal, so it forwards frames the
    /// host pushes and translates them into the same stream messages a local
    /// subscription produces. `project_path` on every forwarded frame is the root
    /// the *remote* resolved, never a local path.
    async fn serve_paired_dag_stream<R, W>(
        &self,
        reader: &mut R,
        write_half: &mut W,
        workspace_id: String,
        binding: crate::daemon::protocol::PairedDagBinding,
    ) where
        R: tokio::io::AsyncBufReadExt + Unpin,
        W: tokio::io::AsyncWriteExt + Unpin,
    {
        use futures_util::StreamExt;

        let stream = crate::paired_host::client::MachineClient::new()
            .attach_dag(
                &self.paired_hosts,
                binding.host_id.clone(),
                binding.generation,
                &binding.remote_workspace_id,
            )
            .await;
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(code = %error.code, workspace_id = %workspace_id, "Paired DAG attach refused");
                let message = DaemonResponse::Error {
                    message: error.code.clone(),
                    code: Some(error.code),
                    details: None,
                };
                if let Ok(mut json) = serde_json::to_string(&message) {
                    json.push('\n');
                    let _ = write_half.write_all(json.as_bytes()).await;
                    let _ = write_half.flush().await;
                }
                return;
            }
        };

        let mut client_lines = reader.lines();
        loop {
            tokio::select! {
                frame = stream.socket.next() => {
                    let Some(Ok(frame)) = frame else { break };
                    let text = match frame {
                        tokio_tungstenite::tungstenite::Message::Text(text) => text,
                        tokio_tungstenite::tungstenite::Message::Close(_) => break,
                        _ => continue,
                    };
                    let Ok(parsed) = serde_json::from_str::<crate::remote::dag_api::DagFrame>(&text) else {
                        continue;
                    };
                    let message = match parsed {
                        crate::remote::dag_api::DagFrame::DagInventory { project_path, runs, .. } => {
                            DaemonStreamMessage::DagInventory {
                                workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
                                project_path: std::borrow::Cow::Owned(project_path),
                                runs,
                            }
                        }
                        crate::remote::dag_api::DagFrame::DagRunUpdated { project_path, snapshot, .. } => {
                            DaemonStreamMessage::DagRunUpdated {
                                workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
                                project_path: std::borrow::Cow::Owned(project_path),
                                snapshot,
                            }
                        }
                        crate::remote::dag_api::DagFrame::DagError { code } => {
                            tracing::warn!(%code, workspace_id = %workspace_id, "Paired DAG stream reported an error");
                            break;
                        }
                    };
                    if let Ok(mut json) = serde_json::to_string(&message) {
                        json.push('\n');
                        if write_half.write_all(json.as_bytes()).await.is_err()
                            || write_half.flush().await.is_err()
                        {
                            break;
                        }
                    }
                }
                line = client_lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            if let Ok(DaemonRequest::UnsubscribeDag { .. }) = serde_json::from_str::<DaemonRequest>(&line) {
                                let mut unsub_ok = serde_json::to_string(&DaemonResponse::UnsubscribeDagOk).unwrap();
                                unsub_ok.push('\n');
                                let _ = write_half.write_all(unsub_ok.as_bytes()).await;
                                let _ = write_half.flush().await;
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
        // Closing the socket is what stops the remote watcher; dropping alone would
        // leave the host streaming until its own transfer timeout.
        let _ = stream.socket.close(None).await;
    }

    async fn serve_dag_stream<R, W>(
        &self,
        reader: &mut R,
        write_half: &mut W,
        workspace_id: String,
        project_path: String,
    ) where
        R: tokio::io::AsyncBufReadExt + Unpin,
        W: tokio::io::AsyncWriteExt + Unpin,
    {
        let ppath = project_path.clone();
        let runs = crate::ipc::run_blocking(move || {
            Ok(crate::daemon::dag_service::scan_project_inventory(
                std::path::Path::new(&ppath),
            ))
        })
        .await
        .unwrap_or_default();

        let inv_msg = DaemonStreamMessage::DagInventory {
            workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
            project_path: std::borrow::Cow::Borrowed(&project_path),
            runs,
        };
        if let Ok(mut inv_json) = serde_json::to_string(&inv_msg) {
            inv_json.push('\n');
            if write_half.write_all(inv_json.as_bytes()).await.is_err()
                || write_half.flush().await.is_err()
            {
                return;
            }
        }

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(100);
        let watcher_handle = crate::dag::watcher::spawn_dag_watcher(
            std::path::PathBuf::from(&project_path),
            event_tx,
        );

        let mut client_lines = reader.lines();
        loop {
            tokio::select! {
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some((_, snapshot)) => {
                            let update_msg = DaemonStreamMessage::DagRunUpdated {
                                workspace_id: std::borrow::Cow::Borrowed(&workspace_id),
                                project_path: std::borrow::Cow::Borrowed(&project_path),
                                snapshot: Box::new(snapshot),
                            };
                            if let Ok(mut update_json) = serde_json::to_string(&update_msg) {
                                update_json.push('\n');
                                if write_half.write_all(update_json.as_bytes()).await.is_err()
                                    || write_half.flush().await.is_err()
                                {
                                    break;
                                }
                            }
                        }
                        None => break,
                    }
                }
                res = client_lines.next_line() => {
                    match res {
                        Ok(Some(line)) => {
                            if let Ok(DaemonRequest::UnsubscribeDag { .. }) = serde_json::from_str::<DaemonRequest>(&line) {
                                let mut unsub_ok = serde_json::to_string(&DaemonResponse::UnsubscribeDagOk).unwrap();
                                unsub_ok.push('\n');
                                let _ = write_half.write_all(unsub_ok.as_bytes()).await;
                                let _ = write_half.flush().await;
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            }
        }
        watcher_handle.abort();
    }

    /// Serves the predecessor's legacy handover socket created by prepare_handover. A
    /// HandoverCandidate started by its own generation job (todo 8) connects here for
    /// DescribeSession / TransferSessions / CommitHandover / AbortHandover; after commit only
    /// sessions this daemon still owns are served on it.
    #[cfg(unix)]
    pub(crate) fn serve_legacy_handover_socket(self: Arc<Self>, listener: UnixListener) {
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let s = Arc::clone(&self);
                tokio::spawn(async move {
                    s.handle_client(stream).await;
                });
            }
        });
    }

    /// Serves the data protocol on `/tmp/rorca-{uid}/daemon.sock` for one release, so a GUI from
    /// before the rename still reaches this daemon. `inherited` is the listener a predecessor from
    /// before the rename handed over. Otherwise the socket is bound right away when the legacy
    /// `daemon.lock` is free, or once a daemon from before the rename that still holds it exits.
    /// Public accepts follow the canonical socket's gate, and a commit hands the legacy lock and
    /// path to the successor.
    #[cfg(unix)]
    pub(crate) fn spawn_legacy_socket_listener(
        self: Arc<Self>,
        inherited: Option<std::os::unix::net::UnixListener>,
    ) {
        let Some(legacy_socket) = get_legacy_socket_path() else {
            return;
        };
        if legacy_socket == get_socket_path() {
            return;
        }
        // On commit the successor takes the legacy path over: release the lock it waits for.
        self.handover_manager.on_commit(|| {
            LEGACY_COMPAT_LOCK.lock().take();
        });
        let runtime = tokio::runtime::Handle::current();
        let listener = match inherited {
            Some(listener) => Some(listener),
            None => match self.bind_legacy_socket(&legacy_socket, false) {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::warn!(%error, path = %legacy_socket.display(), "Legacy daemon socket not served");
                    return;
                }
            },
        };
        if let Some(listener) = listener {
            self.serve_legacy_socket(&runtime, listener, &legacy_socket);
            return;
        }
        // A plain thread, not the blocking pool: the lock wait can last for the daemon's whole
        // life, and runtime shutdown must not wait for it.
        let spawned = std::thread::Builder::new()
            .name("legacy-socket".to_string())
            .spawn(move || match self.bind_legacy_socket(&legacy_socket, true) {
                Ok(Some(listener)) => self.serve_legacy_socket(&runtime, listener, &legacy_socket),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, path = %legacy_socket.display(), "Legacy daemon socket not served");
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "Could not start the legacy daemon socket thread");
        }
    }

    #[cfg(unix)]
    fn serve_legacy_socket(
        self: Arc<Self>,
        runtime: &tokio::runtime::Handle,
        listener: std::os::unix::net::UnixListener,
        path: &Path,
    ) {
        let _entered = runtime.enter();
        let listener = match UnixListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "Failed to serve the legacy daemon socket");
                return;
            }
        };
        tracing::info!(path = %path.display(), "Also serving the pre-rename daemon socket");
        runtime.spawn(async move {
            let manager = Arc::clone(&self.handover_manager);
            loop {
                // None: this daemon handed over; the successor binds the legacy path itself.
                let Some(accepted) = manager.gated_accept(|| listener.accept()).await else {
                    drop(listener);
                    break;
                };
                match accepted {
                    Ok((stream, _)) => {
                        let server = Arc::clone(&self);
                        tokio::spawn(async move {
                            server.handle_client(stream).await;
                        });
                    }
                    Err(error) => {
                        tracing::error!("Legacy daemon socket accept error: {error}");
                        break;
                    }
                }
            }
        });
    }

    /// Takes the legacy lock (see [`take_legacy_lock`]), then binds the legacy socket with the
    /// same 0700 dir and 0600 socket modes as the canonical one. Ok(None) when the lock is held
    /// and `wait` is false, or when this daemon handed its listeners over while it waited.
    /// Blocking.
    #[cfg(unix)]
    fn bind_legacy_socket(
        &self,
        legacy_socket: &Path,
        wait: bool,
    ) -> Result<Option<std::os::unix::net::UnixListener>, String> {
        let dir = legacy_socket
            .parent()
            .ok_or_else(|| format!("{} has no parent", legacy_socket.display()))?;
        ensure_runtime_directory(dir)?;
        let Some(lock) = take_legacy_lock(&dir.join("daemon.lock"), wait)? else {
            return Ok(None);
        };
        if !self.handover_manager.holds_lock_files() {
            return Ok(None);
        }
        remove_stale_socket_after_lock(legacy_socket)?;
        let listener = std::os::unix::net::UnixListener::bind(legacy_socket)
            .map_err(|error| format!("Failed to bind {}: {error}", legacy_socket.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("Failed to configure the legacy socket: {error}"))?;
        validate_safe_ownership_and_type(legacy_socket, RuntimeNodeKind::Socket)?;
        fs::set_permissions(legacy_socket, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Failed to secure the legacy socket: {error}"))?;
        self.note_published_endpoint(legacy_socket);
        *LEGACY_COMPAT_LOCK.lock() = Some(lock);
        Ok(Some(listener))
    }

    /// Takes over the authority of a predecessor from before the rename, whose locks are the
    /// persistent lock and the legacy dir's `daemon.lock`. The persistent lock plus a freshly
    /// taken canonical `daemon.lock` become this daemon's lock files, the legacy lock is kept for
    /// the legacy socket, and the returned legacy listener keeps serving older GUIs. Its control
    /// and agent-state listeners are closed; this daemon binds its own in the new runtime dir.
    #[cfg(unix)]
    fn install_cross_rename_authority(
        &self,
        authority: crate::daemon::handover_socket::TransferredAuthority,
    ) -> Result<std::os::unix::net::UnixListener, String> {
        let adopted = crate::daemon::handover_socket::adopt_validated_authority(authority)
            .map_err(|error| format!("Failed to adopt the pre-rename authority: {error}"))?;
        let (persistent, legacy) = adopted.lock_files.detach_without_unlock();
        let canonical_lock = lock_file_with_optional_wait(
            &get_lock_path(),
            crate::daemon::handover::successor_wait_from_env()
                .map(|wait| std::time::Instant::now() + wait),
        )?;
        self.handover_manager.set_lock_files(DaemonLockFiles::from_locked_files(
            persistent,
            canonical_lock.detach_without_unlock(),
        ));
        *LEGACY_COMPAT_LOCK.lock() = Some(DaemonLockFile::from_locked_file(legacy));
        drop(adopted.control_listener);
        drop(adopted.agent_state_listener);
        tracing::info!("Adopted a pre-rename predecessor; serving its socket as the legacy path");
        Ok(adopted.canonical_listener)
    }

    /// Takes over a validated authority after the predecessor's decision is Retired: installs
    /// the inherited locks, parks the control and agent-state listeners for their serve paths,
    /// and returns the canonical data listener. Never binds and never locks.
    #[cfg(unix)]
    fn install_adopted_authority(
        &self,
        authority: crate::daemon::handover_socket::TransferredAuthority,
    ) -> Result<std::os::unix::net::UnixListener, String> {
        let adopted = crate::daemon::handover_socket::adopt_validated_authority(authority)
            .map_err(|error| format!("Failed to adopt the committed authority: {error}"))?;
        self.handover_manager.set_lock_files(adopted.lock_files);
        self.handover_manager
            .stash_inherited_listeners(adopted.control_listener, adopted.agent_state_listener);
        tracing::info!("Adopted the predecessor's listeners and locks without a bind or lock gap");
        Ok(adopted.canonical_listener)
    }

    /// After a committed decision the transferred local PTYs belong to the successor: drop this
    /// daemon's registry entries without touching the child (the reader stays parked and is
    /// released, the master fd copy is closed). Only the retained SSH/paired sessions remain, so
    /// Draining counts nothing but this daemon's own sessions.
    async fn release_transferred_locals(&self) {
        for session_id in self.handover_manager.transferred_locals() {
            if self.terminal_service.get_session(&session_id).is_none() {
                continue;
            }
            if let Err(error) = self
                .terminal_service
                .pty_manager()
                .relinquish_transferred_session(&session_id)
                .await
            {
                tracing::warn!(%session_id, %error, "Could not drop the transferred session entry");
            }
            self.session_service.session_metadata.write().remove(&session_id);
        }
    }

    /// A paired session id is handled by this daemon's own paired runtime unless an older
    /// generation kept it across an upgrade and still serves it (todo 8): then it is routed.
    fn serves_paired_here(&self, session_id: &str) -> bool {
        crate::terminal::paired_runtime::Runtime::owns(session_id)
            && self.session_router.find_legacy_peer_for_session(session_id).is_none()
    }

    /// Publishes owner.json naming this daemon; failures are retried on a 1 s task.
    fn spawn_owner_publication(
        self: Arc<Self>,
        paths: crate::daemon::owner_admission::AdmissionPaths,
    ) -> tokio::task::JoinHandle<u32> {
        use crate::daemon::owner_admission::{
            current_generation_key, publish_owner, publish_owner_until_published,
            OWNER_PUBLISH_RETRY,
        };
        let epoch = self.epoch;
        tokio::spawn(async move {
            let generation_key = crate::ipc::run_blocking(|| Ok(current_generation_key()))
                .await
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
            publish_owner_until_published(
                || {
                    let paths = paths.clone();
                    let generation_key = generation_key.clone();
                    let fault = self.owner_publish_fault();
                    async move {
                        if fault {
                            return Err("Injected owner.json publication failure".to_string());
                        }
                        crate::ipc::run_blocking(move || {
                            if owner_publish_held() {
                                return Err(crate::ipc::IpcError::internal(
                                    "Injected owner.json publication hold",
                                ));
                            }
                            publish_owner(&paths, &generation_key, epoch)
                                .map_err(crate::ipc::IpcError::internal)
                        })
                        .await
                        .map_err(|error| error.to_string())
                    }
                },
                || -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
                    Box::pin(tokio::time::sleep(OWNER_PUBLISH_RETRY))
                },
            )
            .await
        })
    }

    /// Consumes one injected owner.json publication failure, if armed (QA:
    /// FERRYX_QA_OWNER_PUBLISH_FAILURES=n under local-split-qa).
    fn owner_publish_fault(&self) -> bool {
        #[cfg(feature = "local-split-qa")]
        {
            static REMAINING: std::sync::OnceLock<std::sync::atomic::AtomicU32> =
                std::sync::OnceLock::new();
            let remaining = REMAINING.get_or_init(|| {
                std::sync::atomic::AtomicU32::new(
                    std::env::var("FERRYX_QA_OWNER_PUBLISH_FAILURES")
                        .ok()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                )
            });
            remaining
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |left| left.checked_sub(1),
                )
                .is_ok()
        }
        #[cfg(not(feature = "local-split-qa"))]
        {
            false
        }
    }

    /// Protocol-5 UpgradeBinary path kept unchanged for pre-plan GUIs: the successor is this
    /// daemon's own child. GUIs of this plan never take it; they use control-v1 RequestUpgrade.
    #[cfg(unix)]
    pub fn spawn_legacy_handover_daemon(
        self: Arc<Self>,
        legacy_path: PathBuf,
        listener: UnixListener,
        exe: PathBuf,
    ) {
        // The successor's stdout is `/dev/null` and its stderr is the pipe this process inherited,
        // so nothing it prints reaches a terminal: the durable log file is the only record of what
        // it was asked to take over.
        tracing::info!(
            successor = %exe.display(),
            legacy_socket = %legacy_path.display(),
            "Spawning successor daemon for handover"
        );
        tokio::spawn(async move {
            // Capture the successor's output to a durable log. With `Stdio::null()` a failed
            // handover left no record at all, which is exactly the state a lost-session
            // investigation needs and cannot get after the fact.
            let successor_log = Some(get_runtime_dir().join("handover-successor.log"));
            let successor_stdout = successor_log
                .as_ref()
                .and_then(|path| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .ok()
                })
                .map(std::process::Stdio::from);
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("--daemon")
                .arg("--handover-from")
                .arg(&legacy_path)
                .stdin(std::process::Stdio::null());
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            crate::daemon::qa_producers::inject_handover_fault(
                crate::ipc::qa_barrier::active_channel().as_deref(),
                &mut cmd,
            );
            // The successor must outlive this predecessor, which retires as soon as the transfer
            // commits; a successor left inside the predecessor's process group would be killed by
            // whatever teardown reaches that group.
            crate::util::detach_launched_child(&mut cmd);
            // Lets the successor take over as the systemd unit's main process once it commits.
            crate::daemon::supervisor_linux::forward_notify_socket(&mut cmd);
            match successor_stdout {
                Some(target) => {
                    cmd.stdout(target);
                }
                None => {
                    cmd.stdout(std::process::Stdio::null());
                }
            }
            if let Err(e) = cmd.spawn() {
                // The canonical listener is already gone by this point. Abandoning the loop
                // here would strand every live session with no reachable socket, so keep
                // serving the legacy peer instead of returning.
                tracing::error!(
                    "Failed to spawn new daemon for handover: {e}; continuing to serve legacy socket"
                );
            }

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&self);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
    }

    #[cfg(unix)]
    async fn handle_upgrade_binary(
        self: &Arc<Self>,
        new_binary_path: Option<String>,
    ) -> DaemonResponse {
        let Some(target_exe) = resolve_upgrade_target_exe(new_binary_path.as_deref()) else {
            tracing::error!(
                "Daemon upgrade aborted: no launchable executable on disk (requested {:?}, current_exe {:?})",
                new_binary_path,
                std::env::current_exe().ok()
            );
            return daemon_error(
                "Daemon upgrade unavailable: the running daemon's executable no longer \
                          exists on disk and no valid newBinaryPath was supplied",
            );
        };

        // If no explicit new binary path was provided by the GUI, check if upgrade is needed
        if new_binary_path.is_none() {
            let booted_mtime = self.binary_mtime_ms;
            let target_mtime = tokio::task::spawn_blocking({
                let exe = target_exe.clone();
                move || get_file_mtime_ms(&exe)
            })
            .await
            .unwrap_or(None);

            if !crate::daemon::client::should_request_upgrade(
                None,
                env!("CARGO_PKG_VERSION"),
                booted_mtime,
                target_mtime,
            ) {
                return DaemonResponse::UpgradeNotNeeded;
            }
        }

        let active_sessions = self.terminal_service.list_sessions();
        // One line per accepted request, so a handover that fires twice stays visible: the desktop's
        // staleness probe and the installer both pass their own binary path, while `None` means the
        // caller left the choice of executable to this daemon.
        tracing::info!(
            requested_binary = ?new_binary_path,
            active_sessions = active_sessions.len(),
            "UpgradeBinary accepted"
        );
        if active_sessions.is_empty() {
            let exe = target_exe.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if let Err(e) = perform_daemon_exec_with_path(&exe) {
                    tracing::error!("Failed to re-exec daemon during upgrade: {e}");
                }
            });
            return DaemonResponse::UpgradeScheduled;
        }

        let (legacy_path, _, listener) = match self
            .handover_manager
            .prepare_handover(&self.terminal_service)
        {
            Ok(res) => res,
            Err(e) => return handover_failure_response(Some("Failed to prepare handover"), e),
        };

        Arc::clone(self).spawn_legacy_handover_daemon(legacy_path, listener, target_exe);

        DaemonResponse::UpgradeScheduled
    }

    #[cfg(not(unix))]
    async fn handle_upgrade_binary(
        self: &Arc<Self>,
        _new_binary_path: Option<String>,
    ) -> DaemonResponse {
        let live_sessions = self.terminal_service.list_sessions().len();
        match crate::daemon::handover::upgrade_action(live_sessions, false) {
            crate::daemon::handover::UpgradeAction::Proceed => {
                if !crate::daemon::handover::idle_upgrade_requested() {
                    tracing::warn!(
                        live_sessions,
                        "Daemon upgrade requested on a platform without session transfer; \
                         Ferryx must be restarted to finish updating (set \
                         FERRYX_DAEMON_IDLE_UPGRADE=1 to allow an idle daemon restart)"
                    );
                    return DaemonResponse::UpgradeUnsupported;
                }
                let exe = match std::env::current_exe() {
                    Ok(exe) => exe,
                    Err(error) => {
                        tracing::warn!(%error, "Daemon upgrade aborted: current executable unknown");
                        return DaemonResponse::UpgradeUnsupported;
                    }
                };
                // The successor waits for our instance lock, so it must be running before we
                // release it. No session is live here, so the exit cannot cost a terminal.
                let mut successor = std::process::Command::new(&exe);
                successor
                    .arg("--daemon")
                    .env("FERRYX_DAEMON_SUCCESSOR", "1")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                crate::util::detach_launched_child(&mut successor);
                let spawned = successor.spawn();
                match spawned {
                    Ok(child) => {
                        tracing::warn!(
                            pid = child.id(),
                            live_sessions,
                            "Restarting idle daemon: successor spawned, releasing locks and exiting"
                        );
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            std::process::exit(0);
                        });
                        DaemonResponse::UpgradeScheduled
                    }
                    Err(error) => {
                        tracing::warn!(%error, live_sessions, "Idle daemon restart failed to spawn a successor");
                        DaemonResponse::UpgradeUnsupported
                    }
                }
            }
            crate::daemon::handover::UpgradeAction::Refuse { live_sessions } => {
                // `UpgradeUnsupported` carries no field, so the restart instruction has to
                // travel in the log: this is the only place the user learns why the update
                // did not finish.
                tracing::warn!(
                    live_sessions,
                    "Daemon upgrade refused: this platform cannot transfer live sessions; \
                     restart Ferryx to finish updating"
                );
                DaemonResponse::UpgradeUnsupported
            }
        }
    }

    pub fn handle_register_workspace(
        &self,
        workspace_id: &str,
        repo_root: &str,
    ) -> Result<(), String> {
        self.session_service
            .workspace_service
            .register(workspace_id, repo_root)
    }

    pub async fn handle_remote_configure(
        &self,
        mut config: RemoteGatewayConfig,
    ) -> Result<(), String> {
        config.port = REMOTE_GATEWAY_PORT;
        refresh_cors_saved_host_origins(self.paired_hosts.clone(), Arc::clone(&self.remote_state))
            .await;
        self.configure_gateway(config).await
    }

    pub(crate) async fn configure_gateway(
        &self,
        config: RemoteGatewayConfig,
    ) -> Result<(), String> {
        let _configure = self.remote_configure_lock.lock().await;
        let prev_config = self.remote_state.config.read().clone();
        let bound_address = self.remote_state.bound_address.read().clone();
        let bound_address = bound_address
            .as_deref()
            .and_then(|address| address.parse::<std::net::SocketAddr>().ok());
        if prev_config.mode == RemoteNetworkMode::Relay
            && config.mode == RemoteNetworkMode::Relay
            && (config.port == prev_config.port
                || bound_address.is_some_and(|address| address.port() == config.port))
            && self.remote_server_handle.lock().is_some()
        {
            let address = bound_address
                .ok_or_else(|| "Remote gateway listener address unavailable".to_string())?;
            let client = RemoteServerHandle::prepare_relay(
                Arc::clone(&self.remote_state), config.relay_url.as_deref(), address,
            ).await?;
            let state = Arc::clone(&self.remote_state);
            let handles = Arc::clone(&self.remote_server_handle);
            return crate::ipc::run_blocking(move || {
                let mut handles = handles.lock();
                let handle = handles.as_mut().ok_or_else(|| {
                    crate::ipc::IpcError::internal("Remote gateway stopped during configuration")
                })?;
                *state.config.write() = config;
                if let Err(error) = state.persist_config() {
                    *state.config.write() = prev_config;
                    return Err(crate::ipc::IpcError::internal(format!(
                        "Failed to persist remote gateway config: {error}"
                    )));
                }
                handle.replace_relay(state, client);
                Ok(())
            }).await.map_err(|error| error.to_string());
        }
        if config.mode == RemoteNetworkMode::Off {
            let prev_handle = self.remote_server_handle.lock().take();
            if let Some(handle) = prev_handle {
                handle.stop();
            }
            *self.remote_state.config.write() = config;
            *self.remote_state.is_running.write() = false;
            *self.remote_state.bound_address.write() = None;
            self.remote_state
                .persist_config()
                .map_err(|error| format!("Failed to persist remote gateway config: {error}"))?;
            return Ok(());
        }

        let prev_running = *self.remote_state.is_running.read();
        let prev_address = self.remote_state.bound_address.read().clone();
        *self.remote_state.config.write() = config.clone();

        match start_remote_server(Arc::clone(&self.remote_state)).await {
            Ok((handle, _addr)) => {
                let prev_handle = self.remote_server_handle.lock().take();
                if let Some(h) = prev_handle {
                    h.stop();
                }
                *self.remote_server_handle.lock() = Some(handle);
                self.remote_state
                    .persist_config()
                    .map_err(|error| format!("Failed to persist remote gateway config: {error}"))?;
                Ok(())
            }
            Err(err) => {
                *self.remote_state.config.write() = prev_config;
                *self.remote_state.is_running.write() = prev_running;
                *self.remote_state.bound_address.write() = prev_address;
                Err(err)
            }
        }
    }

    pub async fn pump_sequenced_stream<W>(
        session_id: String,
        rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        Self::pump_sequenced_stream_with_agent_state(
            session_id,
            rx,
            hub,
            writer,
            None,
            None::<tokio::io::Empty>,
            None,
        )
        .await
    }

    async fn pump_session_stream<W, R>(
        &self,
        session_id: String,
        rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
        agent_rx: Option<AgentStateSubscription>,
        client_reader: Option<R>,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
        R: tokio::io::AsyncRead + Unpin + Send,
    {
        // Captured before streaming: the PTY leaves the registry as soon as it exits, which is
        // exactly when the exit frame that must carry this incarnation is written.
        let incarnation = self
            .terminal_service
            .get_session(&session_id)
            .and_then(|session| session.incarnation().map(str::to_owned));
        let paired = self.terminal_service.paired();
        if let Some(mut updates) = paired.subscribe_recovery_outcome(&session_id) {
            let Some(descriptor) = paired.descriptor(&session_id) else { return; };
            let initial = updates.borrow().clone();
            if descriptor.target.daemon_epoch.0.to_string() != initial.daemon_epoch { return; }
            let (input, output) = tokio::io::duplex(64 * 1024);
            let pump = Self::pump_sequenced_stream_with_agent_state(
                session_id.clone(), rx, hub, output, agent_rx, client_reader, incarnation,
            );
            tokio::pin!(pump);
            let mut lines = BufReader::new(input).lines();
            let mut writer = BufWriter::new(writer);
            loop {
                let outcome = updates.borrow_and_update().clone();
                if outcome.backend_session_id != initial.backend_session_id
                    || outcome.daemon_epoch != initial.daemon_epoch
                    || outcome.incarnation != initial.incarnation { return; }
                let state = match outcome.state.as_str() {
                    "proxy_attached" => crate::terminal::remote::RemoteConnectionState::Connected,
                    "affirmative_exit" => crate::terminal::remote::RemoteConnectionState::Expired,
                    "blocked" => crate::terminal::remote::RemoteConnectionState::Disconnected,
                    _ => crate::terminal::remote::RemoteConnectionState::Reconnecting,
                };
                if state == crate::terminal::remote::RemoteConnectionState::Connected && outcome.generation.is_none() {
                    return;
                }
                let failure = outcome
                    .blocked_reason
                    .as_ref()
                    .map(|reason| crate::terminal::remote::RemoteFailure {
                        kind: crate::terminal::remote::RemoteFailureKind::Disconnected,
                        message: reason.clone(),
                    })
                    .or_else(|| {
                        (outcome.state == "blocked").then(|| crate::terminal::remote::RemoteFailure {
                            kind: crate::terminal::remote::RemoteFailureKind::Disconnected,
                            message: "blocked".to_string(),
                        })
                    });
                let status = DaemonStreamMessage::RemoteStatus {
                    session_id: Cow::Borrowed(&session_id),
                    state,
                    generation: outcome.generation.unwrap_or(0),
                    failure,
                    replay_gap: None,
                    descriptor: Some(serde_json::json!({
                        "backendSessionId": outcome.backend_session_id,
                        "hostId": descriptor.host_id,
                        "target": descriptor.target,
                        "incarnation": outcome.incarnation,
                        "daemonEpoch": outcome.daemon_epoch,
                    })),
                };
                let Ok(frame) = crate::daemon::protocol::encode_daemon_stream_frame(&status) else { return; };
                if writer.write_all(frame.as_bytes()).await.is_err() || writer.flush().await.is_err() { return; }
                loop {
                    tokio::select! {
                        _ = &mut pump => { let _ = writer.flush().await; return; },
                        changed = updates.changed() => { if changed.is_err() { let _ = writer.flush().await; return; } break; },
                        line = lines.next_line() => {
                            let Ok(Some(mut line)) = line else { let _ = writer.flush().await; return; };
                            line.push('\n');
                            if writer.write_all(line.as_bytes()).await.is_err() { return; }
                            // Flush once no further complete line is already buffered from the
                            // pump, so a burst goes out in one write and nothing waits unflushed.
                            if !lines.get_ref().buffer().contains(&b'\n') && writer.flush().await.is_err() { return; }
                        }
                    }
                }
            }
        }
        let Ok(mut updates) = self.terminal_service.remote().subscribe(&session_id) else {
            Self::pump_sequenced_stream_with_agent_state(
                session_id,
                rx,
                hub,
                writer,
                agent_rx,
                client_reader,
                incarnation,
            )
            .await;
            return;
        };
        let (input, output) = tokio::io::duplex(64 * 1024);
        let pump = Self::pump_sequenced_stream_with_agent_state(
            session_id.clone(),
            rx,
            hub,
            output,
            agent_rx,
            client_reader,
            incarnation,
        );
        tokio::pin!(pump);
        let mut lines = BufReader::new(input).lines();
        let mut writer = BufWriter::new(writer);
        loop {
            let details = updates.borrow_and_update().clone();
            let status = DaemonStreamMessage::RemoteStatus {
                session_id: Cow::Borrowed(&session_id),
                state: details.state,
                generation: details.generation,
                failure: details.failure,
                replay_gap: details.replay_gap,
                descriptor: Some(serde_json::json!(details.descriptor)),
            };
            let Ok(frame) = crate::daemon::protocol::encode_daemon_stream_frame(&status) else {
                break;
            };
            if writer.write_all(frame.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                break;
            }
            loop {
                tokio::select! {
                    _ = &mut pump => { let _ = writer.flush().await; return; },
                    changed = updates.changed() => { if changed.is_err() { let _ = writer.flush().await; return; } break; },
                    line = lines.next_line() => {
                        let Ok(Some(mut line)) = line else { let _ = writer.flush().await; return; };
                        line.push('\n');
                        if writer.write_all(line.as_bytes()).await.is_err() { return; }
                        // Flush once no further complete line is already buffered from the
                        // pump, so a burst goes out in one write and nothing waits unflushed.
                        if !lines.get_ref().buffer().contains(&b'\n') && writer.flush().await.is_err() { return; }
                    }
                }
            }
        }
    }

    pub(crate) async fn pump_sequenced_stream_with_agent_state<W, R>(
        session_id: String,
        mut rx: broadcast::Receiver<crate::terminal::output_hub::OutputChunk>,
        hub: Arc<TerminalOutputHub>,
        writer: W,
        mut agent_state_rx: Option<AgentStateSubscription>,
        mut client_reader: Option<R>,
        incarnation: Option<String>,
    ) where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
        R: tokio::io::AsyncRead + Unpin + Send,
    {
        let mut writer = BufWriter::new(writer);
        if let Some(snapshot) = agent_state_rx
            .as_ref()
            .and_then(|subscription| subscription.snapshot.as_ref())
        {
            let msg = DaemonStreamMessage::AgentState {
                session_id: Cow::Borrowed(&snapshot.session_id),
                state: Cow::Borrowed(&snapshot.state),
                agent: snapshot.agent.as_deref().map(Cow::Borrowed),
                provider_session: snapshot.provider_session.clone(),
                detail: snapshot.detail.as_deref().map(Cow::Borrowed),
                is_snapshot: true,
                origin: snapshot.origin,
            };
            let Ok(frame) = crate::daemon::protocol::encode_daemon_stream_frame(&msg) else {
                return;
            };
            if writer.write_all(frame.as_bytes()).await.is_err() || writer.flush().await.is_err() {
                return;
            }
        }
        let mut last_seen_sequence: Option<u64> = None;
        // Reused serialization buffer: one allocation serves the whole stream instead of a
        // fresh String per frame (audit H2: optimized payload encoding).
        let mut frame_buf = Vec::with_capacity(8 * 1024);
        // A signal drained from `rx` (lag or close) that must be processed by the main
        // loop on the next iteration instead of inside the batch drain.
        let mut pending: Option<
            Result<crate::terminal::output_hub::OutputChunk, broadcast::error::RecvError>,
        > = None;

        let mut disconnect_buf = [0u8; 1];

        loop {
            let received = match pending.take() {
                Some(received) => received,
                None => {
                    tokio::select! {
                        biased;
                        disconnect = async {
                            match client_reader.as_mut() {
                                Some(r) => r.read(&mut disconnect_buf).await,
                                None => std::future::pending().await,
                            }
                        } => {
                            match disconnect {
                                Ok(0) | Err(_) => {
                                    tracing::debug!(session_id = %session_id, "Client disconnected from attach stream");
                                    break;
                                }
                                Ok(_) => {
                                    // Unexpected client data on output stream; terminate stream
                                    tracing::debug!(session_id = %session_id, "Unexpected client data on attach stream");
                                    break;
                                }
                            }
                        }
                        report = async {
                            match agent_state_rx.as_mut() {
                                Some(subscription) => subscription.receiver.recv().await,
                                None => std::future::pending().await,
                            }
                        } => {
                            match report {
                                Ok(report) if report.state.session_id == session_id => {
                                    let msg = DaemonStreamMessage::AgentState {
                                        session_id: Cow::Borrowed(&session_id),
                                        state: Cow::Borrowed(&report.state.state),
                                        agent: report.state.agent.as_deref().map(Cow::Borrowed),
                                        provider_session: report.state.provider_session,
                                        detail: report.state.detail.as_deref().map(Cow::Borrowed),
                                        is_snapshot: report.is_snapshot,
                                        origin: report.state.origin,
                                    };
                                    frame_buf.clear();
                                    if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                                        break;
                                    }
                                    frame_buf.push(b'\n');
                                    if writer.write_all(&frame_buf).await.is_err() {
                                        break;
                                    }
                                    if writer.flush().await.is_err() {
                                        break;
                                    }
                                }
                                Ok(_) => {}
                                Err(broadcast::error::RecvError::Lagged(_)) => {
                                    if let Some(subscription) = agent_state_rx.as_mut() {
                                        if let Some(current) = subscription.resynchronize(&session_id) {
                                            let msg = DaemonStreamMessage::AgentState {
                                                session_id: Cow::Borrowed(&session_id),
                                                state: Cow::Borrowed(&current.state),
                                                agent: current.agent.as_deref().map(Cow::Borrowed),
                                                provider_session: current.provider_session,
                                                detail: current.detail.as_deref().map(Cow::Borrowed),
                                                is_snapshot: true,
                                                origin: current.origin,
                                            };
                                            frame_buf.clear();
                                            if serde_json::to_writer(&mut frame_buf, &msg).is_err() { break; }
                                            frame_buf.push(b'\n');
                                            if writer.write_all(&frame_buf).await.is_err() || writer.flush().await.is_err() { break; }
                                        }
                                    }
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                            continue;
                        }
                        output = rx.recv() => output,
                    }
                }
            };

            match received {
                Ok(chunk) => {
                    if let Some(gap) = &chunk.replay_gap {
                        let msg = DaemonStreamMessage::Gap {
                            session_id: Cow::Borrowed(&session_id),
                            requested_after_sequence: gap.requested_after_sequence,
                            available_from_sequence: gap.available_from_sequence,
                        };
                        let Ok(frame) = crate::daemon::protocol::encode_daemon_stream_frame(&msg)
                        else {
                            break;
                        };
                        if writer.write_all(frame.as_bytes()).await.is_err()
                            || writer.flush().await.is_err()
                        {
                            break;
                        }
                        last_seen_sequence = Some(chunk.sequence);
                        continue;
                    }
                    if last_seen_sequence.is_some_and(|last| chunk.sequence <= last) {
                        continue;
                    }
                    last_seen_sequence = Some(chunk.sequence);
                    {
                        let msg = DaemonStreamMessage::Output {
                            session_id: Cow::Borrowed(&session_id),
                            sequence: chunk.sequence,
                            data: Cow::Borrowed(&chunk.bytes),
                            metrics_read_unix_micros: chunk.metrics_read_unix_micros,
                        };
                        frame_buf.clear();
                        if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                            break;
                        }
                        frame_buf.push(b'\n');
                        if writer.write_all(&frame_buf).await.is_err() {
                            break;
                        }
                    }

                    // Drain-then-flush (audit M5): absorb any immediately-available output
                    // chunks into the buffered writer before flushing, so a burst costs one
                    // flush per batch instead of one per chunk. Flushing when the queue is
                    // empty keeps interactive latency at a single batch interval.
                    const BATCH_FLUSH_BUDGET_BYTES: usize = 64 * 1024;
                    let mut batched_bytes = frame_buf.len();
                    let mut write_failed = false;
                    while batched_bytes < BATCH_FLUSH_BUDGET_BYTES {
                        match rx.try_recv() {
                            Ok(next) => {
                                if next.replay_gap.is_some() {
                                    pending = Some(Ok(next));
                                    break;
                                }
                                if last_seen_sequence.is_some_and(|last| next.sequence <= last) {
                                    continue;
                                }
                                last_seen_sequence = Some(next.sequence);
                                let msg = DaemonStreamMessage::Output {
                                    session_id: Cow::Borrowed(&session_id),
                                    sequence: next.sequence,
                                    data: Cow::Borrowed(&next.bytes),
                                    metrics_read_unix_micros: next.metrics_read_unix_micros,
                                };
                                frame_buf.clear();
                                if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                                    write_failed = true;
                                    break;
                                }
                                frame_buf.push(b'\n');
                                batched_bytes += frame_buf.len();
                                if writer.write_all(&frame_buf).await.is_err() {
                                    write_failed = true;
                                    break;
                                }
                            }
                            Err(broadcast::error::TryRecvError::Empty) => break,
                            Err(broadcast::error::TryRecvError::Closed) => {
                                pending = Some(Err(broadcast::error::RecvError::Closed));
                                break;
                            }
                            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                                pending = Some(Err(broadcast::error::RecvError::Lagged(n)));
                                break;
                            }
                        }
                    }
                    if write_failed {
                        break;
                    }
                    if writer.flush().await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Re-subscribe with sequence after last_seen_sequence to recover replay gap
                    if let Some((mut att, history_ranges)) =
                        hub.subscribe_with_sequence_ranges(&session_id, last_seen_sequence)
                    {
                        rx = att.receiver;
                        // Captured before `last_seen_sequence` is overwritten below: a delta claim
                        // is only meaningful relative to what this subscriber actually asked for.
                        // A `None` request is a full-history replay, never a delta, no matter what
                        // the snapshot's gap field says.
                        let requested_cursor = last_seen_sequence;
                        let requested_after_sequence = last_seen_sequence.unwrap_or(0);
                        if att.snapshot.history_end_sequence.is_some() {
                            last_seen_sequence = att.snapshot.history_end_sequence;
                        }
                        let available_from_sequence = att
                            .snapshot
                            .gap
                            .as_ref()
                            .map(|gap| gap.available_from_sequence)
                            .or(att.snapshot.history_start_sequence)
                            .unwrap_or_else(|| requested_after_sequence.saturating_add(1));
                        let lag_history = Bytes::from(std::mem::take(&mut att.snapshot.history));
                        let segments = history_ranges
                            .iter()
                            .map(|r| HistorySegmentWire {
                                cols: r.cols,
                                rows: r.rows,
                                bytes: lag_history.slice(r.start..r.end),
                            })
                            .collect();
                        let msg = DaemonStreamMessage::Lagged {
                            session_id: Cow::Borrowed(&session_id),
                            requested_after_sequence,
                            available_from_sequence,
                            start_sequence: att.snapshot.history_start_sequence,
                            end_sequence: att.snapshot.history_end_sequence,
                            history: lag_history,
                            segments,
                            replay_is_delta: Some(
                                requested_cursor.is_some() && att.snapshot.gap.is_none(),
                            ),
                        };
                        frame_buf.clear();
                        if serde_json::to_writer(&mut frame_buf, &msg).is_err() {
                            break;
                        }
                        frame_buf.push(b'\n');
                        if writer.write_all(&frame_buf).await.is_err() {
                            break;
                        }
                        if writer.flush().await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let msg = DaemonStreamMessage::Exit {
                        session_id: Cow::Borrowed(&session_id),
                        exit_code: None,
                        incarnation: incarnation.as_deref().map(Cow::Borrowed),
                    };
                    frame_buf.clear();
                    if serde_json::to_writer(&mut frame_buf, &msg).is_ok() {
                        frame_buf.push(b'\n');
                        let _ = writer.write_all(&frame_buf).await;
                        let _ = writer.flush().await;
                    }
                    break;
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
#[path = "ssh_survival_tests.rs"]
mod ssh_survival_tests;

#[cfg(all(test, unix))]
#[path = "remote_ssh_tests.rs"]
mod remote_ssh_tests;

#[cfg(test)]
mod live_relay_apply_tests {
    use super::*;

    struct Gateway(DaemonServer, [tokio::net::TcpListener; 2]);

    impl Drop for Gateway {
        fn drop(&mut self) {
            if let Some(handle) = self.0.remote_server_handle.lock().take() {
                handle.stop();
            }
        }
    }

    async fn fixture(root: &Path) -> (Gateway, std::net::SocketAddr, String) {
        let server = Gateway(DaemonServer::new_with_paths(
            Some(root.join("config.json")), Some(root.join("auth.json")),
        ), [
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(),
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(),
        ]);
        server.0.configure_gateway(RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay,
            port: 0,
            relay_url: Some(format!("http://{}", server.1[0].local_addr().unwrap())),
            ..Default::default()
        }).await.unwrap();
        let address = server.0.remote_state.bound_address.read().as_ref()
            .unwrap().parse().unwrap();
        let pin = server.0.remote_state.auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, _) = server.0.remote_state.auth_manager
            .exchange_pairing_code(&pin, "relay-apply").unwrap();
        (server, address, token)
    }

    async fn assert_listener(address: std::net::SocketAddr, token: &str) {
        let client = reqwest::Client::builder().no_proxy()
            .timeout(Duration::from_secs(5)).build().unwrap();
        assert_eq!(client.get(format!("http://{address}/api/v1/health"))
            .send().await.unwrap().status(), reqwest::StatusCode::OK);
        assert_eq!(client.get(format!("http://{address}/api/v1/devices"))
            .bearer_auth(token).send().await.unwrap().status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn same_port_relay_apply_preserves_listener_and_auth() {
        let root = tempfile::tempdir().unwrap();
        let (server, address, token) = fixture(root.path()).await;
        let previous_epoch = server.0.remote_state.relay_pairing.read().as_ref().unwrap().epoch;
        assert_listener(address, &token).await;
        let mut config = server.0.remote_state.config.read().clone();
        config.port = address.port();
        let next_url = format!("http://{}", server.1[1].local_addr().unwrap());
        config.relay_url = Some(next_url.clone());

        server.0.configure_gateway(config).await.unwrap();

        assert!(*server.0.remote_state.is_running.read());
        assert_eq!(server.0.remote_state.bound_address.read().as_deref(), Some(address.to_string().as_str()));
        assert_ne!(server.0.remote_state.relay_pairing.read().as_ref().unwrap().epoch, previous_epoch);
        let persisted: serde_json::Value = serde_json::from_slice(
            &fs::read(root.path().join("config.json")).unwrap()).unwrap();
        assert_eq!(persisted["relayUrl"], next_url);
        assert_eq!(persisted["port"], address.port());
        assert_listener(address, &token).await;
    }

    #[tokio::test]
    async fn invalid_relay_apply_preserves_config_publication_and_listener() {
        let root = tempfile::tempdir().unwrap();
        let (server, address, token) = fixture(root.path()).await;
        let previous_epoch = server.0.remote_state.relay_pairing.read().as_ref().unwrap().epoch;
        let previous_file = fs::read(root.path().join("config.json")).unwrap();
        let previous_config = serde_json::to_value(&*server.0.remote_state.config.read()).unwrap();

        for url in ["not a URL", "http://relay.example.com"] {
            let mut config = server.0.remote_state.config.read().clone();
            config.relay_url = Some(url.into());
            assert!(server.0.configure_gateway(config).await.is_err());

            assert_eq!(serde_json::to_value(&*server.0.remote_state.config.read()).unwrap(), previous_config);
            assert_eq!(fs::read(root.path().join("config.json")).unwrap(), previous_file);
            assert_eq!(server.0.remote_state.relay_pairing.read().as_ref().unwrap().epoch, previous_epoch);
            assert!(*server.0.remote_state.is_running.read());
            assert_eq!(server.0.remote_state.bound_address.read().as_deref(), Some(address.to_string().as_str()));
            assert_listener(address, &token).await;
        }
    }
}

fn remote_spawn_relative_path(repo_root: &str, root: &str) -> String {
    let norm_repo = repo_root.replace('\\', "/");
    let norm_root = root.replace('\\', "/");

    let repo_parts: Vec<&str> = norm_repo.split('/').filter(|s| !s.is_empty()).collect();
    let root_parts: Vec<&str> = norm_root.split('/').filter(|s| !s.is_empty()).collect();

    if root_parts.len() < repo_parts.len() {
        return String::new();
    }

    let is_windows = (norm_repo.len() >= 2 && norm_repo.as_bytes()[1] == b':')
        || norm_repo.starts_with("//")
        || (norm_root.len() >= 2 && norm_root.as_bytes()[1] == b':')
        || norm_root.starts_with("//");

    for (repo_part, root_part) in repo_parts.iter().zip(root_parts.iter()) {
        if is_windows {
            if repo_part.to_lowercase() != root_part.to_lowercase() {
                return String::new();
            }
        } else if repo_part != root_part {
            return String::new();
        }
    }

    root_parts[repo_parts.len()..].join("/")
}

#[cfg(all(test, unix))]
mod tests {
    /// A live handover on 2026-09-26 lost 35 of 55 sessions: the successor's receive loop stopped
    /// silently at the first socket error and the mismatch was only warned about, so the commit
    /// went ahead and the predecessor closed the PTY fds of everything it had not delivered.
    #[test]
    fn a_short_delivery_aborts_the_handover_instead_of_committing() {
        // The shape of the incident: 55 offered, 20 delivered.
        let verdict = super::handover_delivery_verdict(55, 20);
        let reason = verdict.expect_err("a short delivery must abort");
        assert!(reason.contains("55"), "names how many were offered: {reason}");
        assert!(reason.contains("20"), "names how many arrived: {reason}");
        assert!(reason.contains("35"), "names how many would be terminated: {reason}");

        // Losing even one session is enough to refuse.
        assert!(super::handover_delivery_verdict(1, 0).is_err());
        assert!(super::handover_delivery_verdict(2, 1).is_err());

        // A complete delivery commits, including the empty case.
        assert!(super::handover_delivery_verdict(0, 0).is_ok());
        assert!(super::handover_delivery_verdict(55, 55).is_ok());
    }

    /// The delivery check above cannot see a session the predecessor never managed to export.
    ///
    /// Observed in production on 2026-09-26: `requested=20 transferred=18`, then
    /// `offered=18 accepted=18`, then commit. The two failed exports were absent from BOTH
    /// sides of the offered/accepted comparison, so the gate reported a clean handover while
    /// those sessions died with the predecessor's PTY master fds.
    #[test]
    fn sessions_that_failed_to_export_also_abort_the_handover() {
        // The exact production shape: owned 20, exported 18, all 18 delivered.
        let verdict = super::handover_delivery_verdict_owned(20, 18, 18);
        let reason = verdict.expect_err("failed exports must abort the handover");
        assert!(reason.contains("20"), "names how many were owned: {reason}");
        assert!(reason.contains("18"), "names how many survived: {reason}");
        assert!(reason.contains("2"), "names how many would be terminated: {reason}");

        // The delivery shortfall is still caught, and reported as a delivery failure.
        assert!(super::handover_delivery_verdict_owned(55, 55, 20).is_err());

        // A predecessor that owned, exported and delivered everything commits.
        assert!(super::handover_delivery_verdict_owned(20, 20, 20).is_ok());
        assert!(super::handover_delivery_verdict_owned(0, 0, 0).is_ok());

        // An older predecessor does not send `requested_count`, which arrives as 0. That must
        // fall back to the delivery check rather than aborting every handover with such a peer.
        assert!(super::handover_delivery_verdict_owned(0, 18, 18).is_ok());
        assert!(super::handover_delivery_verdict_owned(0, 18, 17).is_err());
    }

    #[test]
    fn handover_restorable_unexported_credits_only_listed_unexported_durable_ids() {
        use std::collections::HashSet;

        let listed = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "s1".to_string(),
            "s2".to_string(),
        ];
        let exported: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let durable: HashSet<String> = ["s1", "s2", "x"].iter().map(|s| s.to_string()).collect();

        // listed [a,b,c,s1,s2], exported {a,b,c}, durable {s1,s2,x} gives 2.
        assert_eq!(
            super::handover_restorable_unexported(&listed, &exported, &durable),
            2
        );

        // An id that is exported AND durable is not counted.
        let mut durable_with_a = durable.clone();
        durable_with_a.insert("a".to_string());
        assert_eq!(
            super::handover_restorable_unexported(&listed, &exported, &durable_with_a),
            2
        );

        // A durable id the predecessor did not list is not counted (x).
        assert_eq!(
            super::handover_restorable_unexported(&listed, &exported, &durable),
            2
        );

        // A listed, unexported id that is not durable is not counted.
        let listed_with_unknown = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "s1".to_string(),
            "s2".to_string(),
            "not_durable".to_string(),
        ];
        assert_eq!(
            super::handover_restorable_unexported(&listed_with_unknown, &exported, &durable),
            2
        );

        // A duplicate in listed counts once.
        let listed_with_dups = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "s1".to_string(),
            "s1".to_string(),
            "s2".to_string(),
        ];
        assert_eq!(
            super::handover_restorable_unexported(&listed_with_dups, &exported, &durable),
            2
        );

        // Integration of the numbers:
        // 27 owned, 3 restorable => effective_owned = 24.
        assert!(super::handover_delivery_verdict_owned(27usize.saturating_sub(3), 24, 24).is_ok());
        // 27 owned, 2 restorable => effective_owned = 25 > 24 accepted => abort.
        assert!(super::handover_delivery_verdict_owned(27usize.saturating_sub(2), 24, 24).is_err());
    }

    #[test]
    fn handover_effective_owned_credits_only_an_old_predecessor() {
        // Old predecessor: owned=28, listed_unique=28, restorable=3 gives 25,
        // and handover_delivery_verdict_owned(25, 25, 25).is_ok().
        let effective_old = super::handover_effective_owned(28, 28, 3);
        assert_eq!(effective_old, 25);
        assert!(super::handover_delivery_verdict_owned(effective_old, 25, 25).is_ok());

        // New predecessor: owned=28, listed_unique=60, restorable=32 gives 28.
        let effective_new = super::handover_effective_owned(28, 60, 32);
        assert_eq!(effective_new, 28);

        // New predecessor losing a PTY:
        // handover_delivery_verdict_owned(handover_effective_owned(28, 60, 32), 27, 27).is_err().
        // This is the production bug. It must be err.
        assert!(super::handover_delivery_verdict_owned(effective_new, 27, 27).is_err());

        // Old predecessor losing a PTY as well as the SSH sessions:
        // handover_delivery_verdict_owned(handover_effective_owned(28, 28, 3), 24, 24).is_err().
        assert!(super::handover_delivery_verdict_owned(
            super::handover_effective_owned(28, 28, 3),
            24,
            24
        )
        .is_err());

        // Empty case: owned=0, listed_unique=0, restorable=0 gives 0.
        assert_eq!(super::handover_effective_owned(0, 0, 0), 0);
    }

    use super::*;
    use crate::terminal::output_hub::OutputChunk;
    use tempfile::tempdir;

    #[test]
    fn p08_remote_cwd_aliases_preserve_relative_path() {
        for (repo, cwd, expected) in [
            (r"C:\Repo", r"c:\repo\src", "src"),
            (r"C:\Repo", "C:/Repo/src", "src"),
            (r"C:\Repo", r"c:\repo\deep\nested", "deep/nested"),
            (r"C:\ẞẞ", r"c:\ßß\src", "src"),
            (r"\\host\share\Repo", "//HOST/share/repo/src", "src"),
            ("/repo", "/repo/src", "src"),
            ("/repo", "/repo", ""),
        ] {
            assert_eq!(
                remote_spawn_relative_path(repo, cwd),
                expected,
                "remote cwd {cwd} under {repo} must not silently select repository root"
            );
        }
    }

    #[test]
    fn resolve_upgrade_target_exe_only_resolves_paths_that_exist() {
        let live = std::env::current_exe().expect("test binary path");
        let deleted = PathBuf::from("/nonexistent/Ferryx.app.bak-deleted/Contents/MacOS/ferryx");

        let explicit_wins = {
            let live = live.clone();
            resolve_upgrade_target_exe_with(live.to_str(), || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "current_exe unavailable",
                ))
            })
        };
        assert_eq!(explicit_wins.as_deref(), Some(live.as_path()));

        let falls_back = {
            let live = live.clone();
            resolve_upgrade_target_exe_with(deleted.to_str(), move || Ok(live.clone()))
        };
        assert_eq!(falls_back.as_deref(), Some(live.as_path()));

        // An orphaned daemon: current_exe names an unlinked image and the caller supplied
        // nothing usable. Resolving here would exec a path that cannot be launched.
        let orphaned = {
            let deleted = deleted.clone();
            resolve_upgrade_target_exe_with(None, move || Ok(deleted.clone()))
        };
        assert!(
            orphaned.is_none(),
            "must not resolve to an executable that no longer exists"
        );
    }

    struct AbortTask(tokio::task::JoinHandle<()>);

    impl Drop for AbortTask {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    fn init_test_git_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        dir
    }

    #[tokio::test]
    async fn test_serve_dag_stream_inventory_and_unsubscribe() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let temp = tempdir().expect("tempdir");
        let project_dir = temp.path().to_path_buf();
        let runs_dir = project_dir.join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs dir");

        let run_file = runs_dir.join("run-1.json");
        let run_content =
            include_str!("../dag/testdata/dag_081e597f-0aa8-4a20-a826-4e3d045aacef.json");
        std::fs::write(&run_file, run_content).expect("write run");

        let (mut client_duplex, server_duplex) = tokio::io::duplex(4096);
        let (read_half, mut write_half) = tokio::io::split(server_duplex);
        let mut reader = tokio::io::BufReader::new(read_half);

        let server = Arc::new(DaemonServer::new_with_paths(None, None));
        let ppath = project_dir.to_string_lossy().to_string();
        let server_task = {
            let server = Arc::clone(&server);
            let ppath = ppath.clone();
            tokio::spawn(async move {
                server
                    .serve_dag_stream(&mut reader, &mut write_half, "ws-test".to_string(), ppath)
                    .await;
            })
        };

        let mut client_reader = tokio::io::BufReader::new(&mut client_duplex);
        let mut inv_line = String::new();
        tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            client_reader.read_line(&mut inv_line),
        )
        .await
        .expect("read inventory timeout")
        .expect("read inventory line");

        let inv_msg: DaemonStreamMessage =
            serde_json::from_str(&inv_line).expect("parse inventory");
        match inv_msg {
            DaemonStreamMessage::DagInventory {
                workspace_id,
                project_path,
                runs,
            } => {
                assert_eq!(workspace_id, "ws-test");
                assert_eq!(project_path, ppath);
                assert_eq!(runs.len(), 1);
                assert_eq!(runs[0].run_id, "dag_081e597f-0aa8-4a20-a826-4e3d045aacef");
            }
            _ => panic!("Expected DagInventory message, got: {:?}", inv_msg),
        }

        let unsub_req = DaemonRequest::UnsubscribeDag {
            workspace_id: "ws-test".to_string(),
            project_path: ppath,
        };
        let mut unsub_line = serde_json::to_string(&unsub_req).unwrap();
        unsub_line.push('\n');
        client_reader
            .get_mut()
            .write_all(unsub_line.as_bytes())
            .await
            .expect("write unsub");
        client_reader.get_mut().flush().await.expect("flush unsub");

        tokio::time::timeout(tokio::time::Duration::from_secs(3), async {
            let mut lines = client_reader.lines();
            loop {
                let line = lines.next_line().await.expect("read unsubscribe frame")
                    .expect("stream closed before unsubscribe acknowledgement");
                if matches!(serde_json::from_str::<DaemonResponse>(&line),
                    Ok(DaemonResponse::UnsubscribeDagOk)) {
                    break;
                }
                let frame: DaemonStreamMessage = serde_json::from_str(&line)
                    .expect("expected DAG update before unsubscribe acknowledgement");
                assert!(matches!(frame, DaemonStreamMessage::DagRunUpdated { .. }));
            }
        }).await.expect("unsubscribe acknowledgement timeout");

        tokio::time::timeout(tokio::time::Duration::from_secs(3), server_task)
            .await
            .expect("server task exit timeout")
            .expect("server task join");
    }

    #[tokio::test]
    async fn test_p14_daemon_wires_paired_host_inventory_event_sink() {
        // P14: inventory transitions that originate inside the daemon-owned
        // PairedHostService must reach the desktop through the remote-event
        // broadcast (lib.rs's bridge re-emits paired_host_inventory_changed).
        // The constructor must therefore install the sink in production.
        let server = DaemonServer::new_with_paths(None, None);
        assert!(
            server.paired_hosts.has_event_sink(),
            "daemon-owned PairedHostService must have a production event sink wired"
        );
        assert!(
            server.paired_hosts.has_reachability_sink(),
            "daemon-owned PairedHostService must publish reachability changes"
        );
    }

    #[tokio::test]
    async fn daemon_forwards_global_route_transitions_as_remote_events() {
        use crate::paired_host::direct_route::{DirectGuard, GLOBAL_DIRECT_ROUTES};
        let server = Arc::new(DaemonServer::new_with_paths(None, None));
        let mut remote_events = server.remote_event_tx.subscribe();
        let forwarder = server.spawn_paired_host_route_forwarder();
        // GLOBAL_DIRECT_ROUTES is process-wide; a unique host keeps parallel tests apart.
        let host = format!("route-forwarder-test-{}", std::process::id());
        let epoch = GLOBAL_DIRECT_ROUTES.begin_attempt(&host, std::time::Instant::now()).unwrap();
        assert!(GLOBAL_DIRECT_ROUTES.publish(&host, epoch, "http://127.0.0.1:9".into(), DirectGuard::default()));
        let mut states = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while states.len() < 2 {
                let event = remote_events.recv().await.expect("remote event");
                if event.event == crate::paired_host::route_view::PAIRED_HOST_ROUTE_CHANGED_EVENT
                    && event.payload["hostId"] == host.as_str()
                {
                    states.push(event.payload);
                }
            }
        })
        .await
        .expect("route events forwarded within bound");
        GLOBAL_DIRECT_ROUTES.reset(&host);
        forwarder.abort();
        assert_eq!(
            states,
            vec![
                serde_json::json!({"hostId": host, "state": "attempting"}),
                serde_json::json!({"hostId": host, "state": "direct"}),
            ]
        );
    }

    #[tokio::test]
    async fn test_pump_stream_compact_framing_and_exit() {
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let (_server_read, server_write) = server_stream.into_split();
        let (client_read, _client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        let (tx, rx) = broadcast::channel(16);
        let hub = Arc::new(TerminalOutputHub::default());
        let session_id = "test-session-123".to_string();

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream(
            session_id.clone(),
            rx,
            hub,
            server_write,
        ));

        // Send binary output chunk
        tx.send(OutputChunk {
            sequence: 1,
            bytes: b"hello pty stream\n".to_vec().into(),
            metrics_read_unix_micros: None,
            replay_gap: None,
        })
        .unwrap();

        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        assert!(
            !line.contains('['),
            "Must not contain JSON number array: {line}"
        );
        assert!(
            line.contains(r#""data":"aGVsbG8gcHR5IHN0cmVhbQo=""#),
            "Expected base64 data: {line}"
        );
        assert!(
            line.contains(r#""sessionId":"test-session-123""#),
            "Expected camelCase sessionId: {line}"
        );
        assert!(
            line.contains(r#""sequence":1"#),
            "Expected sequence 1: {line}"
        );

        // Close broadcast sender to trigger Exit
        drop(tx);
        line.clear();
        client_reader.read_line(&mut line).await.unwrap();
        assert!(line.contains(r#""type":"exit""#));
        assert!(line.contains(r#""sessionId":"test-session-123""#));

        let _ = pump_handle.await;
    }

    #[tokio::test]
    async fn exit_frame_carries_the_pty_incarnation() {
        let (client, server_side) = tokio::io::duplex(4096);
        let mut client_reader = BufReader::new(client);
        let (tx, rx) = broadcast::channel(16);
        let hub = Arc::new(TerminalOutputHub::default());

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream_with_agent_state(
            "exiting-session".to_string(),
            rx,
            hub,
            server_side,
            None,
            None::<tokio::io::Empty>,
            Some("incarnation-under-test".to_string()),
        ));

        drop(tx);
        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        let frame: DaemonStreamMessage<'_> = serde_json::from_str(line.trim()).unwrap();
        let DaemonStreamMessage::Exit { incarnation, .. } = frame else {
            panic!("expected an exit frame, got {line}");
        };
        assert_eq!(incarnation.as_deref(), Some("incarnation-under-test"));

        let _ = pump_handle.await;
    }

    #[tokio::test]
    async fn test_server_unregister_workspace_is_idempotent_and_revokes_registration() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        let repo_path = repo.path().to_str().unwrap();

        assert!(server
            .handle_register_workspace("ws-revoke", repo_path)
            .is_ok());
        assert!(server.workspace_registry.contains("ws-revoke"));

        // Revocation removes the binding so the remote gateway (which resolves
        // against this registry) can no longer address the workspace.
        server
            .handle_unregister_workspace("ws-revoke")
            .await
            .expect("unregister of a registered workspace succeeds");
        assert!(!server.workspace_registry.contains("ws-revoke"));

        // Idempotent: unregistering an unknown workspace is a success no-op
        // (e.g. after a daemon restart dropped its registry).
        server
            .handle_unregister_workspace("ws-revoke")
            .await
            .expect("unregister of an unknown workspace is a no-op success");
    }

    #[tokio::test]
    async fn test_server_register_workspace_and_spawn_isolation() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        let repo_path = repo.path().to_str().unwrap();

        // 1. Spawning before registration must fail explicitly (no GUI CWD fallback)
        let unreg_res = server
            .handle_spawn("req-unreg-1", "ws-app", None, None, 80, 24, None, None)
            .await;
        assert!(
            unreg_res.is_err(),
            "Must not infer workspace before explicit registration"
        );

        // 2. Explicit registration of valid canonical git repo
        let reg_res = server.handle_register_workspace("ws-app", repo_path);
        assert!(reg_res.is_ok(), "Registration of valid repo succeeds");

        let nested = repo.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let nested_reg = server.handle_register_workspace("ws-nested", nested.to_str().unwrap());
        assert!(
            nested_reg.is_err(),
            "registration requires the canonical repository root"
        );

        // 3. Spawning in registered workspace succeeds
        let spawn_res = server
            .handle_spawn("req-reg-1", "ws-app", None, None, 80, 24, None, None)
            .await;
        assert!(spawn_res.is_ok());

        // 4. Plain (non-git) directories register as terminal-only workspaces
        let non_git = tempdir().unwrap();
        let plain_reg =
            server.handle_register_workspace("ws-plain", non_git.path().to_str().unwrap());
        assert!(
            plain_reg.is_ok(),
            "plain folders register successfully (terminal-only)"
        );

        // 5. Nonexistent roots still fail registration
        let bad_reg = server.handle_register_workspace("ws-bad", "/definitely/not/a/real/path");
        assert!(bad_reg.is_err());
    }

    #[tokio::test]
    async fn test_server_spawn_idempotency_cache() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let req_id = "spawn-req-idempotency-1".to_string();
        let (first, second) = tokio::join!(
            server.handle_spawn(&req_id, "default", None, None, 80, 24, None, None),
            server.handle_spawn(&req_id, "default", None, None, 80, 24, None, None),
        );
        let session1 = first.expect("first spawn succeeds");
        let session2 = second.expect("concurrent duplicate succeeds");

        assert_eq!(
            session1, session2,
            "Concurrent duplicate requests must share one shell"
        );
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        server
            .terminal_service()
            .close_session(&session1)
            .await
            .expect("close first session");
        let replacement = server
            .handle_spawn(&req_id, "default", None, None, 80, 24, None, None)
            .await
            .expect("dead cached session is never returned");
        assert_ne!(session1, replacement);

        server.expire_spawn_request_for_test(&req_id);
        let after_ttl = server
            .handle_spawn(&req_id, "default", None, None, 80, 24, None, None)
            .await
            .expect("a repeated id never creates a second live shell");
        assert_eq!(replacement, after_ttl);
        assert_eq!(server.spawn_cache_len_for_test(), 1);
    }

    fn claude_resume_startup(id: &str) -> TerminalStartup {
        TerminalStartup::AgentResume {
            agent_type: "claude".to_string(),
            provider_session: crate::daemon::protocol::AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: id.to_string(),
                transcript_path: None,
            },
        }
    }

    #[cfg(unix)]
    fn test_agent_resume_startup(id: &str) -> TerminalStartup {
        TerminalStartup::AgentResume {
            agent_type: "ferryx-test-agent".to_string(),
            provider_session: crate::daemon::protocol::AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: id.to_string(),
                transcript_path: None,
            },
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_agent_resume_production_spawn_path_fences_duplicates_and_conflicts() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let spawn = |request_id: &'static str, barrier: Arc<tokio::sync::Barrier>| {
            let server = Arc::clone(&server);
            async move {
                barrier.wait().await;
                server
                    .handle_spawn(
                        request_id,
                        "default",
                        None,
                        None,
                        80,
                        24,
                        None,
                        Some(test_agent_resume_startup("production-provider")),
                    )
                    .await
            }
        };

        let same_a = spawn("production-same", Arc::clone(&barrier));
        let same_b = spawn("production-same", Arc::clone(&barrier));
        let (_, same_a, same_b) = tokio::join!(barrier.wait(), same_a, same_b);
        let owner = same_a.expect("first production spawn");
        assert_eq!(owner, same_b.expect("idempotent production retry"));
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        let conflict = server
            .handle_spawn(
                "production-competing",
                "default",
                None,
                None,
                80,
                24,
                None,
                Some(test_agent_resume_startup("production-provider")),
            )
            .await
            .expect_err("competing production claim must fail");
        assert!(matches!(conflict, SpawnError::AgentSessionConflict { .. }));
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        let reused_request = server
            .handle_spawn(
                "production-same",
                "default",
                None,
                None,
                100,
                40,
                None,
                Some(test_agent_resume_startup("different-provider")),
            )
            .await
            .expect_err("request id cannot be reused for a different spawn fingerprint");
        assert!(reused_request
            .to_string()
            .contains("reused with a different spawn request"));
        assert_eq!(server.terminal_service().list_sessions().len(), 1);

        server.handle_close(&owner).await.expect("production close");
        assert_eq!(server.provider_claim_len_for_test(), 0);
    }

    #[tokio::test]
    async fn test_agent_resume_idempotency_fingerprint_includes_full_startup_payload() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let startup = |transcript_path: &str| TerminalStartup::AgentResume {
            agent_type: "ferryx-test-agent".to_string(),
            provider_session: crate::daemon::protocol::AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "same-provider-id".to_string(),
                transcript_path: Some(transcript_path.to_string()),
            },
        };

        let first = server
            .handle_spawn(
                "same-request",
                "default",
                None,
                None,
                80,
                24,
                None,
                Some(startup("/tmp/pi-first.json")),
            )
            .await
            .expect("first path-sensitive spawn");
        let changed = server
            .handle_spawn(
                "same-request",
                "default",
                None,
                None,
                80,
                24,
                None,
                Some(startup("/tmp/pi-second.json")),
            )
            .await
            .expect_err("changed startup payload cannot reuse idempotency key");
        assert!(changed
            .to_string()
            .contains("reused with a different spawn request"));

        server
            .handle_close(&first)
            .await
            .expect("close first session");
    }

    #[tokio::test]
    async fn test_agent_resume_identical_request_is_one_spawn() {
        let server = Arc::new(DaemonServer::new());
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let spawn = |barrier: Arc<tokio::sync::Barrier>| {
            let server = Arc::clone(&server);
            async move {
                barrier.wait().await;
                let _guard = server.spawn_lock.lock().await;
                if let Some(meta) = server
                    .session_metadata
                    .read()
                    .iter()
                    .find(|(_, meta)| meta.client_request_id == "resume-same-request")
                    .map(|(session_id, _)| session_id.clone())
                {
                    return Ok(meta);
                }
                server.reserve_provider_claim_for_test(
                    "resume-same-request",
                    "session-one",
                    &claude_resume_startup("provider-same"),
                )
            }
        };
        let first = spawn(Arc::clone(&barrier));
        let second = spawn(Arc::clone(&barrier));
        let (_, first, second) = tokio::join!(barrier.wait(), first, second);
        let first = first.expect("first resume spawn");
        let second = second.expect("duplicate resume spawn");
        assert_eq!(first, second);
        assert_eq!(server.session_metadata.read().len(), 1);
        assert_eq!(server.provider_claim_len_for_test(), 1);
    }

    #[tokio::test]
    async fn test_agent_resume_competing_claim_conflicts_and_releases() {
        let server = Arc::new(DaemonServer::new());
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let spawn = |request_id: &'static str,
                     session_id: &'static str,
                     barrier: Arc<tokio::sync::Barrier>| {
            let server = Arc::clone(&server);
            async move {
                barrier.wait().await;
                let _guard = server.spawn_lock.lock().await;
                server.reserve_provider_claim_for_test(
                    request_id,
                    session_id,
                    &claude_resume_startup("provider-conflict"),
                )
            }
        };
        let first = spawn("resume-owner-a", "session-a", Arc::clone(&barrier));
        let second = spawn("resume-owner-b", "session-b", Arc::clone(&barrier));
        let (_, first, second) = tokio::join!(barrier.wait(), first, second);
        let (owner, conflict) = match (first, second) {
            (Ok(owner), Err(conflict)) | (Err(conflict), Ok(owner)) => (owner, conflict),
            other => panic!("expected one owner and one conflict, got {other:?}"),
        };
        assert!(matches!(conflict, SpawnError::AgentSessionConflict { .. }));
        assert_eq!(server.session_metadata.read().len(), 1);
        assert_eq!(server.provider_claim_len_for_test(), 1);

        server.release_session_ownership(&owner);
        assert_eq!(
            server.provider_claim_len_for_test(),
            0,
            "close/exit cleanup must make the provider reference immediately claimable"
        );
        assert!(
            !server.provider_session_claims.lock().contains_key(
                &ProviderSessionClaimKey::from_startup(Some(&claude_resume_startup(
                    "provider-conflict"
                )))
                .expect("claim key")
            ),
            "released provider claim must not retain a stale owner"
        );
    }

    #[tokio::test]
    async fn test_agent_resume_failed_spawn_does_not_leak_claim() {
        let server = DaemonServer::new();
        let invalid = crate::terminal::shell::resolve_startup_command(
            None,
            Some(&claude_resume_startup("--invalid")),
        );
        assert!(invalid.is_err());
        assert_eq!(server.provider_claim_len_for_test(), 0);

        let valid = server.reserve_provider_claim_for_test(
            "resume-valid",
            "session-valid",
            &claude_resume_startup("provider-after-failure"),
        );
        assert!(valid.is_ok());
        assert_eq!(server.provider_claim_len_for_test(), 1);
    }

    #[tokio::test]
    async fn test_omo_resume_cwd_cannot_escape_workspace() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        let outside = tempdir().unwrap();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let transcript = outside.path().join("session.jsonl");
        fs::write(
            &transcript,
            format!(
                "{}\n",
                serde_json::json!({
                    "type": "session", "id": "provider-session", "cwd": outside.path(),
                })
            ),
        )
        .unwrap();
        let result = server
            .handle_spawn(
                "omo-outside-workspace",
                "default",
                None,
                Some(repo.path().to_string_lossy().into_owned()),
                80,
                24,
                None,
                Some(TerminalStartup::AgentResume {
                    agent_type: "omo".to_string(),
                    provider_session: crate::daemon::protocol::AgentProviderSession {
                        key: AgentProviderSessionKey::SessionId,
                        id: "provider-session".to_string(),
                        transcript_path: Some(transcript.to_string_lossy().into_owned()),
                    },
                }),
            )
            .await;
        assert!(matches!(result, Err(SpawnError::Other(_))));
        assert!(server.terminal_service().list_sessions().is_empty());
    }

    #[tokio::test]
    async fn test_server_spawn_cwd_validation() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        // Nonexistent CWD
        let err_nonexistent = server
            .handle_spawn(
                "req-bad-cwd",
                "default",
                None,
                Some("/nonexistent/path/for/rorca/test/123".to_string()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(err_nonexistent.is_err());
        assert!(err_nonexistent.unwrap_err().contains("CWD does not exist"));

        let outside = tempdir().unwrap();
        let outside_res = server
            .handle_spawn(
                "req-outside-cwd",
                "default",
                None,
                Some(outside.path().to_string_lossy().into_owned()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(
            outside_res.is_err(),
            "arbitrary cwd outside the registered root must fail"
        );

        let symlink_outside = repo.path().join("escape");
        std::os::unix::fs::symlink(outside.path(), &symlink_outside).unwrap();
        let symlink_res = server
            .handle_spawn(
                "req-symlink-cwd",
                "default",
                None,
                Some(symlink_outside.to_string_lossy().into_owned()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(
            symlink_res.is_err(),
            "symlink escape outside the registered root must fail"
        );

        // Valid CWD inside repo
        let ok_cwd = repo.path().to_str().unwrap().to_string();
        let ok_res = server
            .handle_spawn(
                "req-good-cwd",
                "default",
                None,
                Some(ok_cwd.clone()),
                80,
                24,
                None,
                None,
            )
            .await;
        assert!(ok_res.is_ok());

        // Verify DescribeSession reflects cwd
        let session_id = ok_res.unwrap();
        let desc_resp = server.handle_describe_session(&session_id);
        match desc_resp {
            DaemonResponse::DescribeSessionOk { session, .. } => {
                assert_eq!(session.session_id, session_id);
                assert!(session.cwd.is_some());
                assert!(session.running);
            }
            other => panic!("Expected DescribeSessionOk, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_runtime_paths_are_profile_aware() {
        struct RestoreRuntimeDir(Option<std::ffi::OsString>);
        impl Drop for RestoreRuntimeDir {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("FERRYX_RUNTIME_DIR", value),
                    None => std::env::remove_var("FERRYX_RUNTIME_DIR"),
                }
            }
        }
        // This Unix-only test reads no other environment overrides. Restore even
        // on assertion failure; environment-mutating tests run serially.
        let _restore = RestoreRuntimeDir(std::env::var_os("FERRYX_RUNTIME_DIR"));
        std::env::remove_var("FERRYX_RUNTIME_DIR");
        // SAFETY: `getuid` takes no arguments and returns the current process UID.
        let uid = unsafe { libc::getuid() };
        let expected = if cfg!(debug_assertions) {
            PathBuf::from(format!("/tmp/ferryx-{uid}-dev"))
        } else {
            PathBuf::from(format!("/tmp/ferryx-{uid}"))
        };
        assert_eq!(get_runtime_dir(), expected);

        let expected = if cfg!(debug_assertions) {
            PathBuf::from(format!("/tmp/ferryx-{uid}-dev/daemon.sock"))
        } else {
            PathBuf::from(format!("/tmp/ferryx-{uid}/daemon.sock"))
        };
        assert_eq!(get_socket_path(), expected);
        assert_eq!(get_lock_path(), get_runtime_dir().join("daemon.lock"));

        let legacy = if cfg!(debug_assertions) {
            PathBuf::from(format!("/tmp/rorca-{uid}-dev/daemon.sock"))
        } else {
            PathBuf::from(format!("/tmp/rorca-{uid}/daemon.sock"))
        };
        assert_eq!(get_legacy_socket_path(), Some(legacy));
    }

    #[test]
    fn test_persistent_lock_filename_is_profile_aware() {
        let expected = if cfg!(debug_assertions) {
            "daemon-dev.lock"
        } else {
            "daemon.lock"
        };
        assert_eq!(
            get_persistent_lock_path()
                .expect("persistent daemon lock path")
                .file_name()
                .and_then(|name| name.to_str()),
            Some(expected)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_runtime_dir_and_socket_symlink_protection() {
        let dir = tempdir().unwrap();
        let symlink_path = dir.path().join("symlink_dir");
        let target_dir = dir.path().join("target_dir");
        fs::create_dir_all(&target_dir).unwrap();
        std::os::unix::fs::symlink(&target_dir, &symlink_path).unwrap();

        let check = validate_safe_ownership_and_type(&symlink_path, RuntimeNodeKind::Directory);
        assert!(check.is_err(), "Symlinks in runtime dir must be rejected");
        assert!(
            symlink_path.exists(),
            "Validation must never delete an unsafe path"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_runtime_node_modes_are_enforced() {
        let dir = tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_runtime_directory(&runtime).expect("secure runtime directory");
        assert_eq!(
            fs::symlink_metadata(&runtime).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let lock = runtime.join("daemon.lock");
        let _file = open_secure_lock_file(&lock).expect("secure lock file");
        assert_eq!(
            fs::symlink_metadata(&lock).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let socket_symlink = runtime.join("daemon.sock");
        let target = runtime.join("target");
        fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, &socket_symlink).unwrap();
        assert!(remove_stale_socket_after_lock(&socket_symlink).is_err());
        assert!(socket_symlink.symlink_metadata().is_ok());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn test_persistent_daemon_lock_survives_legacy_tmp_lock_replacement() {
        let dir = tempdir().unwrap();
        let persistent = dir.path().join("data").join("locks").join("daemon.lock");
        let legacy_dir = dir.path().join("runtime");
        fs::create_dir(&legacy_dir).unwrap();
        let legacy = legacy_dir.join("daemon.lock");

        let first = acquire_daemon_locks(Some(&persistent), &legacy).expect("first lock set");
        fs::remove_file(&legacy).expect("unlink legacy lock");
        fs::write(&legacy, b"").expect("recreate legacy lock inode");

        let second = acquire_daemon_locks(Some(&persistent), &legacy);
        assert!(second.is_err(), "persistent lock must reject split brain");
        drop(first);
    }

    #[test]
    fn test_daemon_exclusive_lock_semantics_rejects_duplicates_and_releases_on_drop() {
        let dir = tempdir().unwrap();
        let persistent = dir.path().join("data").join("locks").join("daemon.lock");
        let legacy_dir = dir.path().join("runtime");
        fs::create_dir(&legacy_dir).unwrap();
        let legacy = legacy_dir.join("daemon.lock");

        // 1. Dual lock acquisition succeeds for first instance
        let first = acquire_daemon_locks(Some(&persistent), &legacy).expect("first lock set");

        // 2. Second instance attempting dual lock is rejected deterministically without sleeps
        let second_dual = acquire_daemon_locks(Some(&persistent), &legacy);
        assert!(
            second_dual.is_err(),
            "second daemon instance must be rejected while first instance holds lock"
        );
        assert_eq!(
            second_dual.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );

        // 3. Second instance attempting legacy-only lock is also rejected deterministically
        let second_legacy = acquire_daemon_locks(None, &legacy);
        assert!(
            second_legacy.is_err(),
            "second daemon instance requesting legacy lock must be rejected"
        );
        assert_eq!(
            second_legacy.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );

        // 4. Dropping the first lock guard via RAII releases held locks
        drop(first);

        // 5. Subsequent dual lock acquisition now succeeds
        let third = acquire_daemon_locks(Some(&persistent), &legacy)
            .expect("subsequent dual lock acquisition after RAII drop");

        // 6. Collision rejected while third is held
        let third_collision = acquire_daemon_locks(None, &legacy);
        assert!(third_collision.is_err());
        assert_eq!(
            third_collision.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );

        drop(third);

        // 7. Legacy-only lock acquisition succeeds after third is dropped
        let fourth = acquire_daemon_locks(None, &legacy)
            .expect("legacy-only acquisition after RAII release");
        let fourth_collision = acquire_daemon_locks(None, &legacy);
        assert!(fourth_collision.is_err());
        assert_eq!(
            fourth_collision.unwrap_err(),
            "Another daemon instance is already holding the lock."
        );
        drop(fourth);
    }

    #[cfg(unix)]
    #[test]
    fn test_persistent_daemon_lock_creates_only_dedicated_secure_directory() {
        let dir = tempdir().unwrap();
        let data_root = dir.path().join("data");
        fs::create_dir(&data_root).unwrap();
        fs::set_permissions(&data_root, fs::Permissions::from_mode(0o755)).unwrap();
        let persistent = data_root.join("locks").join("daemon.lock");
        let runtime = dir.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        let legacy = runtime.join("daemon.lock");

        let locks = acquire_daemon_locks(Some(&persistent), &legacy).expect("dual locks");
        let data_mode = fs::symlink_metadata(&data_root)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let lock_dir_mode = fs::symlink_metadata(persistent.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(data_mode, 0o755);
        assert_eq!(lock_dir_mode, 0o700);
        drop(locks);
    }

    /// End-to-end proof of the path a phone tap actually travels: HTTP POST on
    /// the real gateway (hosted by this daemon) -> desktop event sink -> the
    /// GUI's subscribed socket connection. Before the sink existed this event
    /// was dropped inside the daemon and the desktop never switched.
    #[tokio::test]
    async fn test_remote_select_request_reaches_a_subscribed_desktop_client() {
        use crate::remote::auth::DevicePermission;
        use crate::remote::server::start_remote_server;
        use crate::remote::state::{RemoteGatewayConfig, RemoteNetworkMode};

        let server = Arc::new(DaemonServer::new());
        *server.remote_state.config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: 0,
            allow_control: true,
            relay_url: None,
        };
        let (handle, addr) = start_remote_server(Arc::clone(&server.remote_state))
            .await
            .expect("start remote gateway");

        // The GUI subscribes over the daemon socket, exactly like lib.rs does.
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let mut json = serde_json::to_string(&DaemonRequest::SubscribeRemoteEvents).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<DaemonResponse>(line.trim()).unwrap(),
            DaemonResponse::SubscribeRemoteEventsOk
        ));

        // Drive the real HTTP endpoint the phone calls, with a paired control
        // device, instead of poking the sink directly.
        let repo = tempfile::tempdir().expect("tempdir");
        server
            .handle_register_workspace("ws", &repo.path().to_string_lossy())
            .expect("register workspace");

        let code = server
            .remote_state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let token = server
            .remote_state
            .auth_manager
            .exchange_pairing_code(&code, "Phone")
            .expect("pair control device")
            .0;
        // No tabId: tab availability is validated against the desktop's last
        // published selection, which is empty in a fresh daemon.
        let body = serde_json::json!({ "workspaceId": "ws" }).to_string();
        // The bearer travels in the Authorization header. A device token in the URL
        // is refused, because it would persist in access logs and browser history.
        let request = format!(
            "POST /api/v1/workspace/select HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut http = tokio::net::TcpStream::connect(addr).await.expect("connect");
        http.write_all(request.as_bytes()).await.expect("send");

        // The GUI must observe the request that arrived over HTTP. Bounded so a
        // regression fails the test instead of hanging the suite.
        line.clear();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            reader.read_line(&mut line),
        )
        .await
        .expect("desktop event must arrive")
        .expect("read event line");
        assert!(read > 0, "subscription closed before delivering the event");
        let event: DaemonRemoteEvent = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event.event, "remote_selection_requested");
        assert_eq!(event.payload["workspaceId"], "ws");

        handle.stop();
    }

    #[tokio::test]
    async fn test_subscribe_remote_events_streams_desktop_directed_events() {
        let server = Arc::new(DaemonServer::new());
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let mut json = serde_json::to_string(&DaemonRequest::SubscribeRemoteEvents).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<DaemonResponse>(line.trim()).unwrap(),
            DaemonResponse::SubscribeRemoteEventsOk
        ));

        server.remote_state.emit_desktop_event(
            "remote_selection_requested",
            serde_json::json!({ "workspaceId": "ws", "tabId": "tab:a::leaf:b" }),
        );

        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let event: DaemonRemoteEvent = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(event.event, "remote_selection_requested");
        assert_eq!(event.payload["tabId"], "tab:a::leaf:b");
    }

    #[tokio::test]
    async fn test_daemon_remote_gateway_lifecycle_and_commands() {
        let server = Arc::new(DaemonServer::new());

        // Default: listener is OFF
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // 1. Check default remote status
        let req = DaemonRequest::RemoteGetStatus;
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteStatusOk { status } => {
                assert_eq!(status.mode, RemoteNetworkMode::Off);
                assert!(!status.is_running);
            }
            other => panic!("Expected RemoteStatusOk, got {other:?}"),
        }

        // 2. Configure to LocalNetwork and create pairing code
        line.clear();
        let conf_req = DaemonRequest::RemoteConfigure {
            config: crate::remote::state::RemoteGatewayConfig {
                mode: RemoteNetworkMode::LocalNetwork,
                port: 0,
                allow_control: true,
                relay_url: None,
            },
        };
        let mut json = serde_json::to_string(&conf_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::RemoteConfigureOk));

        line.clear();
        let pair_req = DaemonRequest::RemoteCreatePairingCode {
            permission: Some(DevicePermission::Control),
        };
        let mut json = serde_json::to_string(&pair_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemotePairingCodeOk { code, .. } => {
                assert_eq!(code.len(), 6);
            }
            other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
        }

        // 3. Active selection set & get
        line.clear();
        let sel_req = DaemonRequest::RemoteSetActiveSelection {
            ssh_store_path: None,
            selection: Some(crate::remote::protocol::RemoteActiveDesktopSelection {
                workspace_id: Some("ws-desktop".to_string()),
                worktree_slug: None,
                worktree_label: None,
                session_id: Some("session-1".to_string()),
                ..Default::default()
            }),
        };
        let mut json = serde_json::to_string(&sel_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::RemoteSetActiveSelectionOk));

        line.clear();
        let get_sel_req = DaemonRequest::RemoteGetActiveSelection;
        let mut json = serde_json::to_string(&get_sel_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteGetActiveSelectionOk { selection } => {
                assert_eq!(
                    selection.unwrap().workspace_id,
                    Some("ws-desktop".to_string())
                );
            }
            other => panic!("Expected RemoteGetActiveSelectionOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_remote_create_pairing_code_relay_unreachable_returns_error() {
        let server = Arc::new(DaemonServer::new());

        // Set mode to Relay with an unreachable relay URL
        *server.remote_state.config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay,
            port: 0,
            relay_url: Some("http://127.0.0.1:1".to_string()),
            ..Default::default()
        };

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let pair_req = DaemonRequest::RemoteCreatePairingCode {
            permission: Some(DevicePermission::Control),
        };
        let mut json = serde_json::to_string(&pair_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::Error { message, .. } => {
                assert!(
                    message.contains("Relay is unreachable")
                        || message.contains("registration failed"),
                    "unexpected error message: {message}"
                );
            }
            other => panic!("Expected Error, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_remote_create_pairing_code_fresh_off_auto_configures_relay() {
        use crate::remote::auth::DeviceAccessScope;

        let temp_dir = tempfile::tempdir().unwrap();
        let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
            vec![],
            temp_dir.path().join("keys.json"),
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_url = format!("http://{}", listener.local_addr().unwrap());
        let relay_task = tokio::spawn(async move {
            axum::serve(
                listener,
                crate::remote::relay_server::relay_router(relay_state)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .expect("serve relay");
        });

        for (pair_req, scope) in [
            (
                DaemonRequest::RemoteCreatePairingCode {
                    permission: Some(DevicePermission::Control),
                },
                DeviceAccessScope::Mirror,
            ),
            (
                DaemonRequest::RemoteCreateMachinePairingCode,
                DeviceAccessScope::Machine,
            ),
        ] {
            let server = Arc::new(DaemonServer::new());
            assert_eq!(
                server.remote_state.config.read().mode,
                RemoteNetworkMode::Off
            );
            // Only substitute the relay endpoint; pairing must start the gateway.
            server.remote_state.config.write().relay_url = Some(relay_url.clone());
            server.remote_state.config.write().port = 0;

            let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
            let server_clone = Arc::clone(&server);
            let server_task = tokio::spawn(async move {
                server_clone.handle_client(server_stream).await;
            });

            let (read_half, mut write_half) = client_stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            let mut json = serde_json::to_string(&pair_req).unwrap();
            json.push('\n');
            write_half.write_all(json.as_bytes()).await.unwrap();

            reader.read_line(&mut line).await.unwrap();
            let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();

            assert_eq!(
                server.remote_state.config.read().mode,
                RemoteNetworkMode::Relay
            );
            assert_eq!(
                server.remote_state.config.read().relay_url,
                Some(relay_url.clone())
            );

            match resp {
                DaemonResponse::RemotePairingCodeOk {
                    code,
                    pairing_token,
                    machine_id,
                    relay_url: effective_url,
                } => {
                    assert!(pairing_token.is_some(), "must not be a local-only PIN");
                    assert!(machine_id.is_some());
                    assert_eq!(effective_url, Some(relay_url.clone()));
                    let (_, device) = server
                        .remote_state
                        .auth_manager
                        .exchange_pairing_code(&code, "Paired device")
                        .expect("pair");
                    assert_eq!(device.access_scope, scope);
                }
                other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
            }

            drop(write_half);
            server_task.await.expect("client task");
        }
        relay_task.abort();
    }

    #[tokio::test]
    async fn test_remote_create_pairing_code_local_mode_creates_local_pin() {
        for mode in [
            RemoteNetworkMode::LocalNetwork,
            RemoteNetworkMode::Tailscale,
        ] {
            let server = Arc::new(DaemonServer::new());
            *server.remote_state.config.write() = RemoteGatewayConfig {
                mode,
                port: 0,
                ..Default::default()
            };

            let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
            let server_clone = Arc::clone(&server);
            let server_task = tokio::spawn(async move {
                server_clone.handle_client(server_stream).await;
            });

            let (read_half, mut write_half) = client_stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            let pair_req = DaemonRequest::RemoteCreatePairingCode {
                permission: Some(DevicePermission::Control),
            };
            let mut json = serde_json::to_string(&pair_req).unwrap();
            json.push('\n');
            write_half.write_all(json.as_bytes()).await.unwrap();

            reader.read_line(&mut line).await.unwrap();
            let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
            match resp {
                DaemonResponse::RemotePairingCodeOk {
                    code,
                    pairing_token,
                    machine_id,
                    relay_url,
                } => {
                    assert_eq!(code.len(), 6);
                    assert_eq!(pairing_token, None);
                    assert_eq!(machine_id, None);
                    assert_eq!(relay_url, None);
                }
                other => panic!("Expected RemotePairingCodeOk for mode {mode:?}, got {other:?}"),
            }
            assert_eq!(server.remote_state.config.read().mode, mode);
            assert_eq!(server.remote_state.config.read().relay_url, None);

            drop(write_half);
            let _ = server_task.await;
        }
    }

    #[tokio::test]
    async fn test_remote_create_pairing_code_with_relay_returns_effective_relay_url() {
        let temp_dir = tempfile::tempdir().unwrap();
        let relay_state = crate::remote::relay_server::RelayState::new_with_key_store(
            vec![],
            temp_dir.path().join("keys.json"),
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_relay_url = format!("http://{}", listener.local_addr().unwrap());
        let relay_task = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                crate::remote::relay_server::relay_router(relay_state)
                    .into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await;
        });

        let server = Arc::new(DaemonServer::new());
        // Configure Relay with the mock relay URL
        let config = RemoteGatewayConfig {
            mode: RemoteNetworkMode::Relay,
            port: 0,
            relay_url: Some(mock_relay_url.clone()),
            ..Default::default()
        };
        server
            .handle_remote_configure(config)
            .await
            .expect("configure gateway");

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let pair_req = DaemonRequest::RemoteCreatePairingCode {
            permission: Some(DevicePermission::Control),
        };
        let mut json = serde_json::to_string(&pair_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemotePairingCodeOk {
                code,
                pairing_token,
                machine_id,
                relay_url,
            } => {
                assert_eq!(code.len(), 6);
                assert!(pairing_token.is_some());
                assert!(machine_id.is_some());
                assert_eq!(relay_url, Some(mock_relay_url));
            }
            DaemonResponse::Error { message, .. } => {
                panic!("Expected RemotePairingCodeOk, got Error: {message}");
            }
            other => panic!("Expected RemotePairingCodeOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
        relay_task.abort();
    }

    #[tokio::test]
    async fn test_stream_lagged_replay_gap_full_recovery() {
        let hub = Arc::new(TerminalOutputHub::new(128));
        let session_id = "test-lag-session".to_string();
        let (_raw_rx, rx) = hub.register_session_channels(&session_id);

        for i in 0..1100 {
            hub.publish(&session_id, format!("chunk_{i:04};").into_bytes());
        }

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let (_server_read, server_write) = server_stream.into_split();
        let (client_read, _client_write) = client_stream.into_split();
        let mut client_reader = BufReader::new(client_read);

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream(
            session_id.clone(),
            rx,
            Arc::clone(&hub),
            server_write,
        ));

        let mut line = String::new();
        client_reader.read_line(&mut line).await.unwrap();
        let message: DaemonStreamMessage<'static> = serde_json::from_str(line.trim()).unwrap();
        match message {
            DaemonStreamMessage::Lagged {
                requested_after_sequence,
                available_from_sequence,
                start_sequence,
                end_sequence,
                history,
                ..
            } => {
                assert_eq!(requested_after_sequence, 0);
                assert!(available_from_sequence > 1);
                assert_eq!(start_sequence, Some(available_from_sequence));
                assert!(end_sequence.unwrap() >= available_from_sequence);
                assert!(!history.is_empty());
                assert!(
                    history.len() <= 128,
                    "replay must stay bounded by hub history"
                );
            }
            other => panic!("Expected typed replayGap frame, got {other:?}"),
        }

        hub.remove_session(&session_id);
        let _ = pump_handle.await;
    }

    #[tokio::test]
    async fn test_server_epoch_in_handshake_and_list() {
        let server = Arc::new(DaemonServer::new());
        assert!(server.epoch() > 0);

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // 1. Handshake
        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: None,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match hs_resp {
            DaemonResponse::HandshakeOk {
                version,
                pid,
                epoch,
                binary_path,
                binary_mtime_ms,
                daemon_version,
                ..
            } => {
                assert_eq!(version, DAEMON_PROTOCOL_VERSION);
                assert_eq!(pid, std::process::id());
                assert_eq!(epoch, server.epoch());
                assert!(binary_path.is_some());
                assert!(binary_mtime_ms.is_some());
                assert!(daemon_version.is_some());
            }
            other => panic!("Expected HandshakeOk, got {other:?}"),
        }

        // 2. ListSessions
        line.clear();
        let list_req = DaemonRequest::ListSessions;
        let mut list_json = serde_json::to_string(&list_req).unwrap();
        list_json.push('\n');
        write_half.write_all(list_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let list_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match list_resp {
            DaemonResponse::ListSessionsOk { epoch, sessions } => {
                assert_eq!(epoch, server.epoch());
                assert!(sessions.is_empty() || !sessions.is_empty());
            }
            other => panic!("Expected ListSessionsOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn owner_identity_local_and_draining_route_keep_distinct_epochs() {
        let mut owner = DaemonServer::new();
        owner.epoch = 9_007_199_254_740_993;
        let owner = Arc::new(owner);
        let repo = init_test_git_repo();
        owner.handle_register_workspace("default", repo.path().to_str().unwrap()).unwrap();
        let session_id = owner.handle_spawn("owner-identity", "default", None, None,
            80, 24, None, None).await.unwrap();
        let local = owner.describe_session_identity(&session_id).await;
        let DaemonResponse::DescribeSessionOk { session: local_details, daemon_epoch } = local else {
            panic!("unexpected local description: {local:?}");
        };
        assert_eq!(daemon_epoch.as_deref(), Some("9007199254740993"));
        assert!(local_details.incarnation.is_some());
        let local_auth = local_details.recovery_authority.as_ref().expect("recovery_authority must be present");
        assert_eq!(local_auth.daemon_epoch.as_deref(), Some("9007199254740993"));
        assert_eq!(local_auth.incarnation, local_details.incarnation);

        let root = tempfile::Builder::new().prefix("fx-owner").tempdir_in("/tmp").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.path().join("owner.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let serving_owner = Arc::clone(&owner);
        let serving = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serving_owner.handle_client(stream).await;
        });
        let mut gateway = DaemonServer::new();
        gateway.epoch = 41;
        gateway.session_router.add_legacy_peer(Arc::new(crate::daemon::proxy::LegacyPeer::new(
            socket, vec![session_id.clone()],
        )));
        let routed = tokio::time::timeout(Duration::from_secs(10),
            gateway.describe_session_identity(&session_id)).await.unwrap();
        let DaemonResponse::DescribeSessionOk { session, daemon_epoch } = routed else {
            panic!("unexpected routed description: {routed:?}");
        };
        assert_eq!(daemon_epoch.as_deref(), Some("9007199254740993"));
        assert_ne!(daemon_epoch, Some(gateway.epoch.to_string()));
        assert_eq!(session.incarnation, local_details.incarnation);
        let routed_auth = session.recovery_authority.as_ref().expect("routed recovery_authority must be present");
        assert_eq!(routed_auth.daemon_epoch.as_deref(), Some("9007199254740993"));
        assert_eq!(routed_auth.incarnation, local_details.incarnation);
        tokio::time::timeout(Duration::from_secs(10), serving).await.unwrap().unwrap();
        owner.handle_close(&session_id).await.unwrap();
    }

    #[tokio::test]
    async fn test_server_handshake_version_mismatch() {
        let server = Arc::new(DaemonServer::new());
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let hs = DaemonRequest::Handshake {
            version: 9999,
            token: None,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match hs_resp {
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => {
                assert_eq!(expected_version, DAEMON_PROTOCOL_VERSION);
                assert_eq!(received_version, 9999);
                assert!(server.terminal_service().list_sessions().is_empty());
            }
            other => panic!("Expected typed ProtocolMismatch, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_server_describe_nonexistent_session() {
        let server = Arc::new(DaemonServer::new());
        let resp = server.handle_describe_session("nonexistent-session-id");
        match resp {
            DaemonResponse::Error { message, .. } => {
                assert!(message.contains("not found"));
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_server_attach_with_after_sequence_and_snapshot() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let session_id = server
            .handle_spawn("req-seq-1", "default", None, None, 80, 24, None, None)
            .await
            .unwrap();

        server
            .terminal_service()
            .output_hub()
            .publish(&session_id, b"chunk1".to_vec());
        server
            .terminal_service()
            .output_hub()
            .publish(&session_id, b"chunk2".to_vec());

        let attachment = server
            .terminal_service()
            .attach_with_sequence(&session_id, Some(1))
            .expect("attach with sequence");

        assert_eq!(attachment.snapshot.session_id, session_id);
        assert_eq!(attachment.snapshot.history, b"chunk2");
        assert_eq!(attachment.snapshot.history_start_sequence, Some(2));
        assert_eq!(attachment.snapshot.history_end_sequence, Some(2));
    }
    #[tokio::test]
    async fn test_server_reset_agent_state_request() {
        let server = Arc::new(DaemonServer::new());
        let session_id = "test-reset-session".to_string();
        server.agent_states.publish_canonical(AgentState {
            session_id: session_id.clone(),
            state: "working".to_string(),
            agent: Some("omo".to_string()),
            provider_session: None,
            detail: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        });
        assert_eq!(
            server.agent_states.current(&session_id).unwrap().state,
            "working"
        );

        // Subscribe to exact state change BEFORE triggering reset
        let mut sub = server.agent_states.subscribe(&session_id);

        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = tokio::io::split(client_stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let req = DaemonRequest::ResetAgentState {
            session_id: session_id.clone(),
        };
        let mut req_json = serde_json::to_string(&req).unwrap();
        req_json.push('\n');

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            write_half.write_all(req_json.as_bytes()).await.unwrap();
            write_half.flush().await.unwrap();
            reader.read_line(&mut line).await.unwrap();
        })
        .await
        .expect("reset request/response within timeout");

        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::ResetAgentStateOk));

        // Await the exact broadcast update with bounded timeout
        let update = tokio::time::timeout(std::time::Duration::from_secs(2), sub.receiver.recv())
            .await
            .expect("update signal within timeout")
            .expect("valid update");
        assert_eq!(update.state.state, "idle");
        assert_eq!(
            server.agent_states.current(&session_id).unwrap().state,
            "idle"
        );

        server_task.abort();
    }

    #[tokio::test]
    async fn agent_state_report_reaches_only_its_own_session_stream() {
        let server = Arc::new(DaemonServer::new());
        let (client, mut server_side) = tokio::io::duplex(4096);
        let session_id = "session-under-test".to_string();
        let other_session = "unrelated-session".to_string();

        let hub = Arc::clone(server.terminal_service().output_hub());
        let (_raw_rx, rx) = hub.register_session_channels(&session_id);
        let agent_rx = server.agent_states.subscribe(&session_id);

        let pump = tokio::spawn(DaemonServer::pump_sequenced_stream_with_agent_state(
            session_id.clone(),
            rx,
            hub,
            client,
            Some(agent_rx),
            None::<tokio::io::Empty>,
            None,
        ));

        // Send the foreign report FIRST, then this session's own report. The pump processes the
        // broadcast in order, so the first frame that arrives is decisive: with a correct filter it
        // is this session's report, and a leak shows up as the foreign one arriving ahead of it.
        // This orders the assertion on the channel itself instead of on elapsed time.
        server.agent_states.publish_canonical(AgentState {
            session_id: other_session.clone(),
            state: "working".to_string(),
            agent: Some("codex".to_string()),
            provider_session: None,
            detail: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        });
        server.agent_states.publish_canonical(AgentState {
            session_id: session_id.clone(),
            state: "blocked".to_string(),
            agent: Some("omo".to_string()),
            provider_session: None,
            detail: None,
            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
        });

        let mut reader = BufReader::new(&mut server_side);
        let mut line = String::new();
        tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            reader.read_line(&mut line),
        )
        .await
        .expect("stream produced a frame")
        .expect("frame read");

        let msg: serde_json::Value = serde_json::from_str(line.trim()).expect("json frame");
        assert_eq!(msg["type"], "agentState");
        assert_eq!(msg["sessionId"], "session-under-test");
        assert_eq!(
            msg["agent"], "omo",
            "the foreign report for {other_session} must be dropped, not relabelled onto this stream"
        );
        assert_eq!(
            msg["state"], "blocked",
            "the reported state must survive the hop verbatim"
        );

        pump.abort();
    }

    #[tokio::test]
    async fn attach_resynchronizes_agent_state_reported_before_subscription() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let session_id = server
            .handle_spawn(
                "req-agent-snapshot",
                "default",
                None,
                None,
                80,
                24,
                None,
                None,
            )
            .await
            .expect("spawn test session");

        let result: Result<serde_json::Value, String> = async {
            // This is the same hub publication used by the production agent-state socket handler.
            // The report deliberately arrives before handle_client subscribes for Attach.
            server.agent_states.publish_canonical(AgentState {
                session_id: session_id.clone(),
                state: "working".to_string(),
                agent: Some("omo".to_string()),
                provider_session: None,
                detail: None,
                origin: crate::daemon::protocol::AgentStateOrigin::Agent,
            });

            let (client_stream, server_stream) = UnixStream::pair().map_err(|e| e.to_string())?;
            let server_task = AbortTask(tokio::spawn(
                Arc::clone(&server).handle_client(server_stream),
            ));
            let (read_half, mut write_half) = client_stream.into_split();
            let mut reader = BufReader::new(read_half);

            for request in [
                DaemonRequest::Handshake {
                    version: DAEMON_PROTOCOL_VERSION,
                    token: None,
                },
                DaemonRequest::Attach {
                    session_id: session_id.clone(),
                    after_sequence: None,
                },
            ] {
                let mut frame = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
                frame.push(b'\n');
                write_half
                    .write_all(&frame)
                    .await
                    .map_err(|e| e.to_string())?;
                write_half.flush().await.map_err(|e| e.to_string())?;

                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| e.to_string())?;
                let response: DaemonResponse =
                    serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                if !matches!(
                    (request, response),
                    (
                        DaemonRequest::Handshake { .. },
                        DaemonResponse::HandshakeOk { .. }
                    ) | (
                        DaemonRequest::Attach { .. },
                        DaemonResponse::AttachOk { .. }
                    )
                ) {
                    return Err("unexpected attach response".to_string());
                }
            }

            let snapshot = tokio::time::timeout(tokio::time::Duration::from_secs(10), async {
                loop {
                    let mut line = String::new();
                    let read = reader
                        .read_line(&mut line)
                        .await
                        .map_err(|e| e.to_string())?;
                    if read == 0 {
                        return Err("attach stream closed before agent snapshot".to_string());
                    }
                    let value: serde_json::Value =
                        serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
                    if value["type"] == "agentState" {
                        return Ok(value);
                    }
                }
            })
            .await
            .map_err(|_| {
                "attach must receive retained agent state without another report".to_string()
            })??;

            drop(write_half);
            drop(server_task);
            Ok(snapshot)
        }
        .await;

        // Cleanup runs before any assertion, including timeout and malformed-frame failures.
        server
            .handle_close(&session_id)
            .await
            .expect("close test session");

        let snapshot = result.expect("attach state snapshot");
        assert_eq!(snapshot["sessionId"], session_id);
        assert_eq!(snapshot["state"], "working");
        assert_eq!(snapshot["isSnapshot"], true);
    }

    #[test]
    fn agent_state_reports_are_parsed_and_filtered() {
        assert_eq!(
            DaemonServer::parse_agent_state_report(
                r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"omo"}"#
            ),
            Some((
                "s1".to_string(),
                "working".to_string(),
                Some("omo".to_string()),
                None,
                None,
            ))
        );
        let valid = DaemonServer::parse_agent_state_report(
            r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"omo","providerSession":{"key":"session_id","id":"omo-session"}}"#,
        )
        .expect("valid Omo provider report");
        assert_eq!(valid.3.expect("provider reference").id, "omo-session");
        let wrong_key = DaemonServer::parse_agent_state_report(
            r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"omo","providerSession":{"key":"conversation_id","id":"bad-key"}}"#,
        )
        .expect("activity report remains valid");
        assert_eq!(
            wrong_key.3, None,
            "semantically invalid provider reference must be omitted"
        );
        let missing_required_path = DaemonServer::parse_agent_state_report(
            r#"{"type":"agentState","sessionId":"s1","state":"working","agent":"pi","providerSession":{"key":"session_id","id":"pi-session"}}"#,
        )
        .expect("activity report remains valid");
        assert_eq!(
            missing_required_path.3, None,
            "path-sensitive provider reference must validate fully"
        );
        assert_eq!(
            DaemonServer::parse_agent_state_report(
                r#"{"type":"agentState","sessionId":"s1","state":"idle"}"#
            ),
            Some(("s1".to_string(), "idle".to_string(), None, None, None)),
            "agent is optional so older extension copies keep working"
        );
        let with_detail = DaemonServer::parse_agent_state_report(
            r#"{"type":"agentState","sessionId":"s1","state":"blocked","agent":"omo","detail":"Auth method — Which library should we use?"}"#,
        )
        .expect("a blocked report carries the question text");
        assert_eq!(with_detail.1, "blocked");
        assert_eq!(
            with_detail.4.as_deref(),
            Some("Auth method — Which library should we use?"),
            "the question text must survive parsing, or the inbox row cannot show it"
        );
        for rejected in [
            r#"{"type":"agentState","sessionId":"s1","state":"bogus"}"#,
            r#"{"sessionId":"s1"}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(
                DaemonServer::parse_agent_state_report(rejected),
                None,
                "must reject {rejected:?}"
            );
        }
    }

    #[test]
    fn test_normalize_process_cwd_windows_verbatim_paths() {
        use super::normalize_process_cwd;
        use std::path::{Path, PathBuf};

        // Verbatim drive paths should have \\?\ stripped for process cwd
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\C:\Windows\System32")),
            PathBuf::from(r"C:\Windows\System32")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\c:\Users\test\project")),
            PathBuf::from(r"c:\Users\test\project")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\D:")),
            PathBuf::from(r"D:")
        );
        // Verbatim UNC paths \\?\UNC\server\share -> \\server\share
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\UNC\server\share\repo")),
            PathBuf::from(r"\\server\share\repo")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\?\unc\server\share")),
            PathBuf::from(r"\\server\share")
        );
        // Non-verbatim UNC, Windows drive, Unix, and relative paths should remain unchanged
        assert_eq!(
            normalize_process_cwd(Path::new(r"\\server\share\repo")),
            PathBuf::from(r"\\server\share\repo")
        );
        assert_eq!(
            normalize_process_cwd(Path::new(r"C:\Windows\System32")),
            PathBuf::from(r"C:\Windows\System32")
        );
        assert_eq!(
            normalize_process_cwd(Path::new("/Users/test/project")),
            PathBuf::from("/Users/test/project")
        );
        assert_eq!(
            normalize_process_cwd(Path::new("relative/path")),
            PathBuf::from("relative/path")
        );
        // Non-drive verbatim path preserved
        assert_eq!(
            normalize_process_cwd(Path::new(
                r"\\?\Volume{b75e2c83-0000-0000-0000-602200000000}\"
            )),
            PathBuf::from(r"\\?\Volume{b75e2c83-0000-0000-0000-602200000000}\")
        );
    }

    // Re-explore-4 G2. Each row names a family; dropping an agent from MACHINE_RESUME_AGENTS or
    // from the observer's decision breaks its row.
    #[test]
    fn machine_identity_probe_runs_for_every_published_family_on_machine_panes_only() {
        for agent in crate::terminal::resume_cwd::MACHINE_RESUME_AGENTS {
            assert!(machine_identity_probe_wanted(agent, true, false), "{agent}");
            // A local pane's UI probes for itself.
            assert!(!machine_identity_probe_wanted(agent, false, false), "{agent}");
            // An identity already present is never replaced.
            assert!(!machine_identity_probe_wanted(agent, true, true), "{agent}");
        }
        for agent in ["kimi", "copilot", "gjc"] {
            assert!(machine_identity_probe_wanted(agent, true, false), "{agent}");
        }
        // Families whose discovery is a cwd guess stay out (C25).
        for agent in ["pi", "antigravity", "opencode", "aider", "unknown"] {
            assert!(!machine_identity_probe_wanted(agent, true, false), "{agent}");
        }
    }

    #[test]
    fn machine_identity_probe_backs_off_and_stops_after_its_budget() {
        assert_eq!(machine_identity_probe_delay(0), Some(MACHINE_IDENTITY_PROBE_FLOOR));
        assert_eq!(
            machine_identity_probe_delay(1),
            Some(MACHINE_IDENTITY_PROBE_FLOOR * 2)
        );
        let mut previous = Duration::ZERO;
        for attempt in 0..MACHINE_IDENTITY_PROBE_ATTEMPTS {
            let delay = machine_identity_probe_delay(attempt).expect("within budget");
            assert!(delay >= previous && delay <= MACHINE_IDENTITY_PROBE_CEILING);
            previous = delay;
        }
        assert_eq!(previous, MACHINE_IDENTITY_PROBE_CEILING);
        assert_eq!(machine_identity_probe_delay(MACHINE_IDENTITY_PROBE_ATTEMPTS), None);
        assert_eq!(machine_identity_probe_delay(u32::MAX), None);
    }

    #[test]
    fn discovered_report_for_kimi_copilot_gjc_is_a_resumable_pathless_claim() {
        // Ids in each family's measured shape (local-2.md:256, local-3.md:17, remote-gjc.md).
        for (agent, id) in [
            ("kimi", "eca66b4f-a89b-4979-9f13-cbe3f35b5991"),
            ("copilot", "5793c06c-5bee-4b34-883b-589051286cfd"),
            ("gjc", "01a16274-abd4-7581-8f9c-22214661ef6a"),
        ] {
            let (session_id, state, reported_agent, provider, detail) =
                discovered_machine_report("pane-1", agent, id, None);
            assert_eq!(session_id, "pane-1");
            assert_eq!(state, "idle", "{agent}: nothing retained reads as idle");
            assert_eq!(reported_agent.as_deref(), Some(agent));
            assert_eq!(detail, None);
            let provider = provider.expect("claim");
            assert_eq!(provider.key, AgentProviderSessionKey::SessionId);
            assert_eq!(provider.id, id);
            // admit_agent_report refuses a pinned path, so a discovered claim carries none.
            assert_eq!(provider.transcript_path, None);
            crate::terminal::shell::resolve_agent_resume_plan(agent, &provider)
                .unwrap_or_else(|error| panic!("{agent} must resume from the id: {error}"));
            // The ingress filter keeps it: the same path an agent's own report takes.
            let admitted = DaemonServer::admit_agent_state_report(AgentStateReport {
                session_id: session_id.clone(),
                state: "idle".into(),
                agent: reported_agent.clone(),
                provider_session: Some(provider.clone()),
                detail: None,
            })
            .expect("valid state");
            assert_eq!(admitted.3, Some(provider), "{agent}");

            // The retained state word and detail are carried, never invented.
            let retained = AgentState {
                session_id: "pane-1".into(),
                state: "blocked".into(),
                agent: Some(agent.into()),
                provider_session: None,
                detail: Some("approve?".into()),
                origin: crate::daemon::protocol::AgentStateOrigin::ProcessObserved,
            };
            let (_, state, _, _, detail) =
                discovered_machine_report("pane-1", agent, id, Some(&retained));
            assert_eq!(state, "blocked");
            assert_eq!(detail.as_deref(), Some("approve?"));
        }
    }

    /// A provider claim shaped the way the extension reports it.
    fn reported_claim(id: &str, path: Option<&str>) -> AgentProviderSession {
        AgentProviderSession {
            key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
            id: id.to_string(),
            transcript_path: path.map(str::to_string),
        }
    }

    #[test]
    fn a_launch_argv_id_never_vetoes_a_switched_claude_or_omp_conversation() {
        use crate::ipc::agents::DiscoverySource;
        let stale = "96b3d95c-0000-4000-8000-000000000000";
        let switched = "2cc12210-0000-4000-8000-000000000000";
        // Measured: after `claude --resume A` then `/clear`, claude writes session B while its
        // argv still names A. For claude and omp the argv id is dropped, so B is admitted on its
        // own store file instead of being refused as a mismatch.
        for agent in ["claude", "omp", "cline", "crush"] {
            let found = binding_discovery(agent, stale.into(), DiscoverySource::LaunchArgv);
            assert_eq!(found, None, "{agent}");
            assert_eq!(
                ingress_admission_identity(agent, switched, true, found.as_deref(), true),
                IngressAdmissionIdentity::WithoutPath,
                "{agent}"
            );
            // A live (open-file) discovery still binds and still vetoes a different id.
            let live = binding_discovery(agent, stale.into(), DiscoverySource::Live);
            assert_eq!(live.as_deref(), Some(stale));
            assert_eq!(
                ingress_admission_identity(agent, switched, true, live.as_deref(), true),
                IngressAdmissionIdentity::Refused
            );
        }
        // Every other agent keeps its argv binding.
        for agent in ["grok", "droid", "kimi", "copilot", "gjc"] {
            assert_eq!(
                binding_discovery(agent, stale.into(), DiscoverySource::LaunchArgv).as_deref(),
                Some(stale),
                "{agent}"
            );
        }
    }

    #[test]
    fn ingress_admits_a_fresh_claude_on_its_own_store_alone() {
        // claude keeps no process-bound record of a fresh session (re-explore-5, measured), so
        // with nothing discovered its own store file for the reported id is the proof.
        let id = "0f3c9a52-6b1e-4d7a-9c2e-8a4b5d6e7f80";
        assert!(agent_has_no_process_bound_session("claude"));
        assert_eq!(
            ingress_admission_identity("claude", id, true, None, true),
            IngressAdmissionIdentity::WithoutPath
        );
        // A foreign path, or a discovered id that differs, is still refused.
        assert_eq!(
            ingress_admission_identity("claude", id, true, None, false),
            IngressAdmissionIdentity::Refused
        );
        assert_eq!(
            ingress_admission_identity(
                "claude",
                id,
                true,
                Some("01a00000-0000-0000-0000-000000000000"),
                true
            ),
            IngressAdmissionIdentity::Refused
        );
        // Every agent that does leave a process-bound record still needs discovery.
        assert!(agent_has_no_process_bound_session("omp"));
        // A fresh cline or crush pane is the same case (re-explore-7 G1/G2, measured).
        for agent in ["cline", "crush"] {
            assert!(agent_has_no_process_bound_session(agent), "{agent}");
        }
        for agent in ["omo", "codex", "grok", "kimi", "copilot", "gjc", "cursor"] {
            assert!(!agent_has_no_process_bound_session(agent), "{agent}");
            assert_eq!(
                ingress_admission_identity(agent, id, true, None, true),
                IngressAdmissionIdentity::Refused,
                "{agent} with nothing discovered must be refused"
            );
        }
    }

    #[test]
    fn ingress_drops_a_verified_path_for_every_machine_published_family() {
        let id = "01a112e9-02b1-7857-b11d-967060362583";
        // The canonical machine OMO report: the id is the one discovery read from the pane's own
        // process, and the path is the store that id resolves to on this host.
        assert_eq!(
            ingress_admission_identity("omo", id, true, Some(id), true),
            IngressAdmissionIdentity::WithoutPath
        );
        // A path that is not this session's own store is refused, never admitted.
        assert_eq!(
            ingress_admission_identity("omo", id, true, Some(id), false),
            IngressAdmissionIdentity::Refused
        );
        // An id the live process does not report is refused.
        assert_eq!(
            ingress_admission_identity(
                "omo",
                id,
                true,
                Some("01a00000-0000-0000-0000-000000000000"),
                true
            ),
            IngressAdmissionIdentity::Refused
        );
        // Nothing discovered means nothing verified.
        assert_eq!(
            ingress_admission_identity("omo", id, true, None, true),
            IngressAdmissionIdentity::Refused
        );
        // Every family the machine publishes can lose a verified path, and loses it only when
        // both proofs hold. Removing a family from the admission breaks its row here.
        for agent in ["omo", "claude", "codex", "grok", "omp"] {
            assert_eq!(
                ingress_admission_identity(agent, id, true, Some(id), true),
                IngressAdmissionIdentity::WithoutPath,
                "{agent} must be admitted without its verified path"
            );
            assert_eq!(
                ingress_admission_identity(agent, id, true, Some(id), false),
                IngressAdmissionIdentity::Refused,
                "{agent} with a foreign path must be refused"
            );
            assert_eq!(
                ingress_admission_identity(
                    agent,
                    id,
                    true,
                    Some("01a00000-0000-0000-0000-000000000000"),
                    true
                ),
                IngressAdmissionIdentity::Refused,
                "{agent} with an id the process does not report must be refused"
            );
        }
        // A family the machine never publishes keeps its claim exactly as reported, so the
        // daemon's existing pinned-path rule still decides it.
        for agent in ["pi", "antigravity", "opencode"] {
            assert_eq!(
                ingress_admission_identity(agent, id, true, Some(id), true),
                IngressAdmissionIdentity::AsReported,
                "{agent} must keep its pinned path"
            );
        }
        // A claim with no path is admitted exactly as reported, whatever the agent.
        assert_eq!(
            ingress_admission_identity("omo", id, false, Some(id), false),
            IngressAdmissionIdentity::AsReported
        );
    }

    /// Writes one store file at `relative` under `home` and returns its path.
    fn store_file(home: &Path, relative: &str) -> PathBuf {
        let path = home.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{}\n").unwrap();
        path
    }

    #[test]
    fn ingress_proves_each_path_reporting_family_against_its_own_store() {
        use crate::scoped_contracts::CanonicalProvider;
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let roots = crate::ipc::agent_history::provider_history_roots_with_env(home, |_| None);
        // The store layouts these agents report as their transcript path (measured fixtures in
        // cli/fixtures/agent_hook and ferryx-agent-state.ts).
        let cases = [
            (
                "claude",
                "d49fdc31-682a-40b4-811a-eea8a17f6cf9",
                ".claude/projects/-tmp-proj/d49fdc31-682a-40b4-811a-eea8a17f6cf9.jsonl",
            ),
            (
                "codex",
                "01a1626f-f0db-75d0-b73c-2372a21d4cae",
                ".codex/sessions/2026/10/22/rollout-2026-10-22T16-07-46-01a1626f-f0db-75d0-b73c-2372a21d4cae.jsonl",
            ),
            (
                "grok",
                "01a16274-a688-7e73-b6fc-434bc7781be4",
                ".grok/sessions/%2Ftmp%2Fproj/01a16274-a688-7e73-b6fc-434bc7781be4/updates.jsonl",
            ),
            (
                "omp",
                "01a16270-e358-7635-88e4-1b2f84460057",
                ".omp/agent/sessions/-tmp-proj/2026-10-22T07-08-47-832Z_01a16270-e358-7635-88e4-1b2f84460057.jsonl",
            ),
        ];
        for (agent, id, relative) in cases {
            let path = store_file(home, relative);
            assert!(
                ingress_path_is_agent_store(agent, id, &path, &roots),
                "{agent} must prove its own store file"
            );
            // The same file never proves a different id.
            assert!(
                !ingress_path_is_agent_store(agent, "01a00000-0000-0000-0000-000000000000", &path, &roots),
                "{agent} must refuse a store file of another session"
            );
            // A file of the same name outside the agent's own root is foreign.
            let foreign = store_file(home, &format!("elsewhere/{relative}"));
            assert!(
                !ingress_path_is_agent_store(agent, id, &foreign, &roots),
                "{agent} must refuse a path outside its own store root"
            );
            // Another family's root never proves this agent's claim.
            let other_roots: Vec<_> = roots
                .iter()
                .filter(|(provider, _)| {
                    crate::agent_history::history::provider_wire(*provider) != agent
                })
                .cloned()
                .collect();
            assert!(
                !ingress_path_is_agent_store(agent, id, &path, &other_roots),
                "{agent} must be proven against its own root only"
            );
        }
        // A claimed path that does not exist, or a directory, proves nothing.
        assert!(!ingress_path_is_agent_store(
            "claude",
            "d49fdc31-682a-40b4-811a-eea8a17f6cf9",
            &home.join(".claude/projects/-tmp-proj/missing.jsonl"),
            &roots
        ));
        assert!(!ingress_path_is_agent_store(
            "grok",
            "01a16274-a688-7e73-b6fc-434bc7781be4",
            &home.join(".grok/sessions/%2Ftmp%2Fproj/01a16274-a688-7e73-b6fc-434bc7781be4"),
            &roots
        ));
        // crush keeps every session in one database: no file proves one session.
        let crush_db = store_file(home, ".local/share/crush/crush.db");
        assert!(!ingress_path_is_agent_store(
            "crush",
            "7f7fbfa62dd136ec",
            &crush_db,
            &[(CanonicalProvider::Crush, home.join(".local/share/crush"))]
        ));
    }

    #[cfg(unix)]
    #[test]
    fn ingress_refuses_a_symlink_into_another_agents_store() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let roots = crate::ipc::agent_history::provider_history_roots_with_env(home, |_| None);
        let id = "d49fdc31-682a-40b4-811a-eea8a17f6cf9";
        // A link spelled like a claude store file, pointing at a codex file: canonicalization
        // resolves it into the codex root, which is not claude's.
        let target = store_file(home, &format!(".codex/sessions/2026/10/22/{id}.jsonl"));
        let link = home.join(format!(".claude/projects/-tmp-proj/{id}.jsonl"));
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(!ingress_path_is_agent_store("claude", id, &link, &roots));
    }

    #[test]
    fn ingress_publishes_local_metadata_and_withholds_refused_identities() {
        let reported = Some(reported_claim("reported-id", Some("/home/qa/.pi/store.jsonl")));
        let admitted = Some(reported_claim("admitted-id", None));
        // A local pane keeps the metadata it reported, path included: its consumer resolves the
        // transcript from that path.
        assert_eq!(
            ingress_publish_identity(IngressOutcome::Local, reported.clone(), admitted.clone()),
            reported
        );
        // An admitted machine session publishes the admitted identity.
        assert_eq!(
            ingress_publish_identity(IngressOutcome::Admitted, reported.clone(), admitted.clone()),
            admitted
        );
        // A refusal publishes NO identity, so a stale or foreign claim is never published.
        assert_eq!(ingress_publish_identity(IngressOutcome::Refused, reported, admitted), None);
        assert_eq!(ingress_publish_identity(IngressOutcome::Refused, None, None), None);
        assert_eq!(ingress_publish_identity(IngressOutcome::Local, None, None), None);
    }

    #[test]
    fn test_binary_identity_helper_pure_and_injection() {
        // Injection test: successful exe and mtime
        let mock_exe = || Ok(PathBuf::from("/opt/ferryx/bin/ferryx"));
        let mock_mtime = |_path: &Path| Some(1725280000000u64);
        let (path, mtime) = resolve_binary_identity_with(mock_exe, mock_mtime);
        assert_eq!(path, Some("/opt/ferryx/bin/ferryx".to_string()));
        assert_eq!(mtime, Some(1725280000000u64));

        // Injection test: exe failure
        let mock_exe_err = || {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "not found",
            ))
        };
        let (path_err, mtime_err) = resolve_binary_identity_with(mock_exe_err, mock_mtime);
        assert_eq!(path_err, None);
        assert_eq!(mtime_err, None);

        // Injection test: mtime failure
        let mock_mtime_none = |_path: &Path| None;
        let (path_no_mtime, mtime_none) = resolve_binary_identity_with(mock_exe, mock_mtime_none);
        assert_eq!(path_no_mtime, Some("/opt/ferryx/bin/ferryx".to_string()));
        assert_eq!(mtime_none, None);

        // Live identity test
        let (live_path, live_mtime) = resolve_binary_identity();
        assert!(live_path.is_some(), "live binary path should be resolved");
        assert!(
            live_mtime.is_some_and(|t| t > 0),
            "live binary mtime should be non-zero"
        );
    }

    #[tokio::test]
    async fn test_handle_upgrade_binary_idempotent_when_not_needed() {
        let server = Arc::new(DaemonServer::new());
        let resp = server.handle_upgrade_binary(None).await;
        // The running test binary is at least as new as itself on disk, so upgrade is not needed
        #[cfg(unix)]
        assert!(matches!(resp, DaemonResponse::UpgradeNotNeeded));
        #[cfg(not(unix))]
        assert!(matches!(resp, DaemonResponse::UpgradeUnsupported));
    }

    #[cfg(unix)]
    #[test]
    fn test_clear_cloexec_and_fd_inheritance_across_exec() {
        use std::io::Write;
        use std::os::unix::io::FromRawFd;

        // 1. Create a pipe
        let mut pipe_fds = [0i32; 2];
        let ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
        assert_eq!(ret, 0);
        let [read_fd, write_fd] = pipe_fds;

        // 2. Set FD_CLOEXEC explicitly on read_fd (simulating portable-pty behavior)
        let set_cloexec_ret = unsafe { libc::fcntl(read_fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        assert_eq!(set_cloexec_ret, 0);

        let flags = unsafe { libc::fcntl(read_fd, libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0, "FD_CLOEXEC must be set");

        // 3. Clear FD_CLOEXEC using clear_cloexec
        clear_cloexec(read_fd).expect("clear_cloexec must succeed");

        let flags_cleared = unsafe { libc::fcntl(read_fd, libc::F_GETFD) };
        assert_eq!(
            flags_cleared & libc::FD_CLOEXEC,
            0,
            "FD_CLOEXEC must be cleared"
        );

        // 4. Write data to write_fd
        let test_message = b"ferryx-inherited-fd-payload\n";
        let mut write_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
        write_file.write_all(test_message).expect("write to pipe");
        drop(write_file); // Close writer so reader hits EOF

        // 5. Spawn /bin/sh to read from inherited read_fd
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("read -r line <&{}; echo \"$line\"", read_fd))
            .output()
            .expect("spawn helper process with inherited fd");

        unsafe {
            libc::close(read_fd);
        }

        assert!(output.status.success());
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout_str.trim(), "ferryx-inherited-fd-payload");
    }

    #[tokio::test]
    async fn test_daemon_production_configure_gated_status_projection() {
        let _guard = crate::remote::server::DIRECT_GATE_TEST_MUTEX.lock().await;
        use crate::remote::server::set_allow_insecure_direct;
        set_allow_insecure_direct(false);

        let server = Arc::new(DaemonServer::new());
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // Configure to LocalNetwork without insecure opt-in
        let conf_req = DaemonRequest::RemoteConfigure {
            config: crate::remote::state::RemoteGatewayConfig {
                mode: RemoteNetworkMode::LocalNetwork,
                port: 0,
                allow_control: true,
                relay_url: None,
            },
        };
        let mut json = serde_json::to_string(&conf_req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::RemoteConfigureOk));

        // Now query RemoteGetStatus: must expose gate status + reason
        line.clear();
        let req = DaemonRequest::RemoteGetStatus;
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::RemoteStatusOk { status } => {
                assert_eq!(status.mode, RemoteNetworkMode::LocalNetwork);
                assert!(status.is_running);
                assert!(
                    matches!(
                        status.gate_status,
                        Some(
                            crate::remote::server::DirectGatewayGateStatus::InsecureLanGated { .. }
                        )
                    ),
                    "expected InsecureLanGated in status response, got {:?}",
                    status.gate_status
                );
                assert!(
                    status
                        .gate_reason
                        .as_deref()
                        .unwrap_or_default()
                        .contains("gated"),
                    "expected gating reason in status response, got {:?}",
                    status.gate_reason
                );
            }
            other => panic!("Expected RemoteStatusOk, got {other:?}"),
        }

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn remote_get_status_follows_the_accepted_control_channel_not_pairing() {
        use super::*;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let server = Arc::new(DaemonServer::new());
        let client = crate::remote::relay_client::RelayClient::new("https://relay.example", "token");
        *server.remote_state.relay_pairing.write() = Some(crate::remote::state::PublishedPairing {
            coordinator: client.pairing_coordinator(),
            epoch: 1,
        });
        *server.remote_state.relay_client.write() = Some(client.clone());

        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });
        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);

        async fn status(write_half: &mut tokio::net::unix::OwnedWriteHalf, reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> DaemonRemoteStatus {
            let mut line = String::new();
            let mut json = serde_json::to_string(&DaemonRequest::RemoteGetStatus).unwrap();
            json.push('\n');
            write_half.write_all(json.as_bytes()).await.unwrap();
            reader.read_line(&mut line).await.unwrap();
            match serde_json::from_str::<DaemonResponse>(line.trim()).unwrap() {
                DaemonResponse::RemoteStatusOk { status } => status,
                other => panic!("Expected RemoteStatusOk, got {other:?}"),
            }
        }

        let before = status(&mut write_half, &mut reader).await;
        assert_eq!(before.relay_connected, Some(false));
        assert_eq!(before.control_channel_connected, Some(false));

        let admission = client.test_admit_control_channel();
        let during = status(&mut write_half, &mut reader).await;
        assert_eq!(during.relay_connected, Some(true));
        assert_eq!(during.control_channel_connected, Some(true));

        drop(admission);
        let after = status(&mut write_half, &mut reader).await;
        assert_eq!(after.relay_connected, Some(false));
        assert_eq!(after.control_channel_connected, Some(false));

        drop(write_half);
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_idle_stream_client_disconnect_terminates_pump() {
        let (client_read_side, server_write_side) = tokio::io::duplex(4096);
        let (server_read_side, client_write_side) = tokio::io::duplex(4096);

        let (_tx, rx) = broadcast::channel(16);
        let hub = Arc::new(TerminalOutputHub::default());
        let session_id = "test-idle-session".to_string();

        let pump_handle = tokio::spawn(DaemonServer::pump_sequenced_stream_with_agent_state(
            session_id,
            rx,
            hub,
            server_write_side,
            None,
            Some(server_read_side),
            None,
        ));

        // Terminal is completely idle: no output sent.
        // Client drops write and read ends of socket (e.g. GUI tab close or abort).
        drop(client_write_side);
        drop(client_read_side);

        // Assert that the pump terminates within 2 seconds instead of hanging forever.
        let result = tokio::time::timeout(Duration::from_secs(2), pump_handle).await;
        assert!(
            result.is_ok(),
            "pump must terminate promptly when client disconnects on idle stream"
        );
    }

    #[tokio::test]
    async fn test_idle_stream_handle_client_attach_disconnect_terminates() {
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let session_id = server
            .handle_spawn("req-idle-1", "default", None, None, 80, 24, None, None)
            .await
            .unwrap();

        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = tokio::io::split(client_stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: None,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();
        write_half.flush().await.unwrap();
        reader.read_line(&mut line).await.unwrap();

        let attach = DaemonRequest::Attach {
            session_id: session_id.clone(),
            after_sequence: None,
        };
        let mut attach_json = serde_json::to_string(&attach).unwrap();
        attach_json.push('\n');
        write_half.write_all(attach_json.as_bytes()).await.unwrap();
        write_half.flush().await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::AttachOk { .. }));

        // Now session is idle (no output produced). Drop client stream.
        drop(reader);
        drop(write_half);

        let result = tokio::time::timeout(Duration::from_secs(2), server_task).await;
        assert!(
            result.is_ok(),
            "handle_client must terminate promptly when client disconnects from idle attach"
        );
    }

    #[tokio::test]
    async fn test_handle_client_upload_clipboard_image() {
        let server = Arc::new(DaemonServer::new_with_paths(None, None));
        let (client_stream, server_stream) = UnixStream::pair().expect("unix pair");
        let server_clone = Arc::clone(&server);
        let _server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = client_stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        let req = DaemonRequest::UploadClipboardImage {
            file_name: "test-client-paste.png".to_string(),
            data: b"fake-daemon-paste-bytes".to_vec(),
        };
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::UploadClipboardImageOk {
                remote_path,
                byte_length,
            } => {
                assert!(remote_path.ends_with("test-client-paste.png"));
                assert_eq!(byte_length, 23);
                let content = std::fs::read(&remote_path).expect("read saved paste file");
                assert_eq!(content, b"fake-daemon-paste-bytes");
                let _ = std::fs::remove_file(remote_path);
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_legacy_peer_attach_and_stream_client_eof_drops_legacy_connection() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::net::UnixListener;

        let socket_dir = tempfile::Builder::new()
            .prefix("fx-legacy")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(socket_dir.path(), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let socket_path = socket_dir.path().join("legacy.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let (legacy_closed_tx, legacy_closed_rx) = tokio::sync::oneshot::channel();
        let legacy_daemon = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();

            // Handshake
            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("handshake"));
            write
                .write_all(b"{\"type\":\"handshakeOk\",\"version\":3,\"pid\":1,\"epoch\":1}\n")
                .await
                .unwrap();

            // Attach
            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("attach"));
            write.write_all(b"{\"type\":\"attachOk\",\"epoch\":1,\"sessionId\":\"legacy-1\",\"startSequence\":null,\"endSequence\":null,\"gap\":null,\"history\":\"\"}\n").await.unwrap();

            // The legacy daemon is now idle, waiting for commands or connection close.
            // When LegacyPeer drops its socket, next_line() returns None (EOF).
            let eof = lines.next_line().await.unwrap();
            assert!(
                eof.is_none(),
                "legacy daemon must observe EOF when client disconnects"
            );
            let _ = legacy_closed_tx.send(());
        });

        let peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
            socket_path,
            vec!["legacy-1".into()],
        ));
        let agent_hub = Arc::new(AgentStateHub::default());
        let agent_sub = agent_hub.subscribe("legacy-1");

        let (client_writer_read, mut client_writer) = tokio::io::duplex(4096);
        let (client_reader_write, mut client_reader) = tokio::io::duplex(4096);

        let peer_clone = Arc::clone(&peer);
        let stream_task = tokio::spawn(async move {
            peer_clone
                .attach_and_stream(
                    "legacy-1",
                    None,
                    &mut client_writer,
                    &mut client_reader,
                    agent_sub,
                    agent_hub,
                )
                .await
        });

        // Client reads the forwarded AttachOk frame
        let mut client_lines = BufReader::new(client_writer_read).lines();
        let first_line = client_lines.next_line().await.unwrap().unwrap();
        assert!(first_line.contains("attachOk"));

        // Now client disconnects (drops write end, sending EOF to client_reader)
        drop(client_reader_write);

        // attach_and_stream must terminate promptly
        let stream_res = tokio::time::timeout(Duration::from_secs(2), stream_task).await;
        assert!(
            stream_res.is_ok(),
            "attach_and_stream must terminate within timeout"
        );
        assert!(stream_res.unwrap().unwrap().is_ok());

        // Legacy daemon must have observed EOF and closed its socket within timeout
        let legacy_res = tokio::time::timeout(Duration::from_secs(2), legacy_closed_rx).await;
        assert!(
            legacy_res.is_ok(),
            "upstream connection to legacy daemon must be closed promptly"
        );

        legacy_daemon.await.unwrap();
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_legacy_peer_attach_session_receiver_drop_terminates_reader() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::net::UnixListener;

        let socket_dir = tempfile::Builder::new()
            .prefix("fx-legacy")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(socket_dir.path(), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let socket_path = socket_dir.path().join("legacy_session.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let (legacy_closed_tx, legacy_closed_rx) = tokio::sync::oneshot::channel();
        let legacy_daemon = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();

            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("handshake"));
            write
                .write_all(b"{\"type\":\"handshakeOk\",\"version\":3,\"pid\":1,\"epoch\":1}\n")
                .await
                .unwrap();

            let line = lines.next_line().await.unwrap().unwrap();
            assert!(line.contains("attach"));
            write.write_all(b"{\"type\":\"attachOk\",\"epoch\":1,\"sessionId\":\"legacy-session-1\",\"startSequence\":1,\"endSequence\":1,\"gap\":null,\"history\":\"\"}\n").await.unwrap();

            // Send output periodically until EOF
            let eof = loop {
                if write.write_all(b"{\"type\":\"output\",\"sessionId\":\"legacy-session-1\",\"sequence\":2,\"data\":\"aGVsbG8=\"}\n").await.is_err() {
                    break true;
                }
                match lines.next_line().await {
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break true,
                }
            };
            assert!(eof);
            let _ = legacy_closed_tx.send(());
        });

        let peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
            socket_path,
            vec!["legacy-session-1".into()],
        ));
        let attachment = peer.attach_session("legacy-session-1", None).await.unwrap();

        // Drop the receiver, making receiver_count() == 0
        drop(attachment.receiver);

        // Within 2 seconds, the legacy connection should be closed because reader task breaks
        let legacy_res = tokio::time::timeout(Duration::from_secs(2), legacy_closed_rx).await;
        assert!(
            legacy_res.is_ok(),
            "dropping receiver must cause legacy peer reader task to drop connection"
        );

        legacy_daemon.await.unwrap();
    }
}

#[cfg(test)]
mod instance_lock_deadline_tests {
    use super::*;

    #[test]
    fn expired_wait_window_never_attempts_or_acquires_instance_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance.lock");

        // 1. Even if the lock file is completely free, calling lock_file_with_optional_wait
        // with an already-expired deadline must return Err immediately without acquiring.
        // If expiry were checked after attempt (the bug), try_lock_file would succeed and return Ok.
        let expired_deadline = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(10))
            .unwrap();
        let outcome = lock_file_with_optional_wait(&path, Some(expired_deadline));
        assert!(
            outcome.is_err(),
            "an expired wait window must never acquire the lock, even when free"
        );

        // 2. Predecessor holds lock past the successor's deadline:
        // Successor must wait the full window and timeout with Err.
        let held = try_lock_file(open_secure_lock_file(&path).unwrap()).unwrap();
        let started = std::time::Instant::now();
        let outcome = lock_file_with_optional_wait(
            &path,
            Some(std::time::Instant::now() + std::time::Duration::from_millis(200)),
        );
        assert!(
            outcome.is_err(),
            "successor must timeout when predecessor holds past deadline"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(180),
            "wait must last until deadline, took {:?}",
            started.elapsed()
        );
        drop(held);

        // 3. Without a declared successor deadline, plain startup fails fast on a held lock.
        let held_again = try_lock_file(open_secure_lock_file(&path).unwrap()).unwrap();
        let fast = std::time::Instant::now();
        let immediate = lock_file_with_optional_wait(&path, None);
        assert!(immediate.is_err(), "plain startup must fail on held lock");
        assert!(
            fast.elapsed() < std::time::Duration::from_millis(100),
            "plain startup must fail fast, took {:?}",
            fast.elapsed()
        );
        drop(held_again);

        // 4. Predecessor releases WITHIN deadline: successor acquires successfully.
        let held_third = try_lock_file(open_secure_lock_file(&path).unwrap()).unwrap();
        let release_thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(held_third);
        });
        let acquired = lock_file_with_optional_wait(
            &path,
            Some(std::time::Instant::now() + std::time::Duration::from_millis(400)),
        );
        assert!(
            acquired.is_ok(),
            "successor must acquire lock when predecessor releases within deadline"
        );
        release_thread.join().unwrap();
        drop(acquired);

        // 5. Pre-check passes with remaining window, but delay before lock crosses deadline:
        // Post-acquisition check deterministically detects late lock, drops it immediately, and returns Err.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(20);
        let late_outcome = lock_file_with_optional_wait_internal(
            &path,
            Some(deadline),
            || std::thread::sleep(std::time::Duration::from_millis(30)),
        );
        assert!(
            late_outcome.is_err(),
            "lock acquired past deadline must be dropped and rejected"
        );
        let err_msg = late_outcome.unwrap_err();
        assert!(
            err_msg.contains("after the wait window expired"),
            "must be rejected specifically by the post-acquisition check, got: {err_msg}"
        );
        // Lock must have been dropped, so an immediate unarmed call succeeds.
        assert!(lock_file_with_optional_wait(&path, None).is_ok());
    }

    #[tokio::test]
    async fn test_desktop_geometry_restore_lifecycle() {
        use crate::remote::backend::RemoteSessionBackend;

        let server = Arc::new(DaemonServer::new());
        let repo = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git").args(["init"]).current_dir(repo.path()).output();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        // 1. Spawn a session initially at 80x24.
        let session_id = server
            .handle_spawn("req-restore-1", "default", None, None, 80, 24, None, None)
            .await
            .unwrap();

        let session = server.terminal_service.get_session(&session_id).unwrap();
        assert_eq!(session.get_size(), (80, 24));

        // 2. Desktop resize to 120x30 recorded via request dispatch.
        server.record_desktop_geometry(&session_id, 120, 30);
        server
            .resize_session(&session_id, 120, 30)
            .await
            .unwrap();

        assert_eq!(
            server.session_service.desktop_geometry(&session_id),
            Some((120, 30))
        );
        assert_eq!(session.get_size(), (120, 30));

        // 3. Remote resize to 40x20 changes PTY size without overwriting desktop recorded geometry.
        server
            .session_service
            .resize(&session_id, 40, 20)
            .await
            .unwrap();
        assert_eq!(session.get_size(), (40, 20));
        assert_eq!(
            server.session_service.desktop_geometry(&session_id),
            Some((120, 30)),
            "remote resize must not overwrite desktop geometry"
        );

        // 4. restore_desktop_geometry restores PTY back to 120x30.
        server
            .session_service
            .restore_desktop_geometry(&session_id)
            .await
            .unwrap();
        assert_eq!(session.get_size(), (120, 30));

        // 5. restore without a record is a no-op.
        server
            .session_service
            .desktop_geometries
            .lock()
            .remove(&session_id);
        assert_eq!(server.session_service.desktop_geometry(&session_id), None);
        server
            .session_service
            .resize(&session_id, 50, 25)
            .await
            .unwrap();
        assert_eq!(session.get_size(), (50, 25));
        server
            .session_service
            .restore_desktop_geometry(&session_id)
            .await
            .unwrap();
        assert_eq!(session.get_size(), (50, 25));

        // 6. Session close cleans up recorded geometry
        server
            .session_service
            .record_desktop_geometry(&session_id, 120, 30);
        assert_eq!(
            server.session_service.desktop_geometry(&session_id),
            Some((120, 30))
        );
        server.handle_close(&session_id).await.unwrap();
        assert_eq!(
            server.session_service.desktop_geometry(&session_id),
            None,
            "session close must remove desktop geometry record"
        );
    }

    #[tokio::test]
    async fn test_daemon_request_resize_records_desktop_geometry() {
        let server = Arc::new(DaemonServer::new());
        let repo = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git").args(["init"]).current_dir(repo.path()).output();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();
        let session_id = server
            .handle_spawn("req-restore-2", "default", None, None, 80, 24, None, None)
            .await
            .unwrap();

        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            server_clone.handle_client(server_stream).await;
        });

        let (read_half, mut write_half) = tokio::io::split(client_stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        #[cfg(not(unix))]
        let token = Some(server.transport_token.clone());
        #[cfg(unix)]
        let token = None;

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();
        write_half.flush().await.unwrap();
        reader.read_line(&mut line).await.unwrap();

        let req = DaemonRequest::Resize {
            session_id: session_id.clone(),
            cols: 140,
            rows: 45,
        };
        let mut json = serde_json::to_string(&req).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.unwrap();
        write_half.flush().await.unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::ResizeOk));

        assert_eq!(
            server.session_service.desktop_geometry(&session_id),
            Some((140, 45)),
            "DaemonRequest::Resize must record desktop geometry"
        );

        drop(reader);
        drop(write_half);
        let _ = server_task.await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn regression_daemon_remote_spawn_2_remote_write_resize_forward_to_legacy_peer() {
        let server = Arc::new(DaemonServer::new());
        let session_id = uuid::Uuid::new_v4().to_string();

        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("fx-legacy")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.path().join("peer.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();

        // LegacyPeer opens a fresh connection (with its own handshake) for every request.
        let peer_task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (reader, mut writer) = tokio::io::split(stream);
                    let mut lines = tokio::io::BufReader::new(reader).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let response = match serde_json::from_str::<DaemonRequest>(&line).unwrap() {
                            DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                                version: DAEMON_PROTOCOL_VERSION,
                                pid: 1,
                                epoch: 1,
                                binary_path: None,
                                binary_mtime_ms: None,
                                daemon_version: None,
                                capabilities: Vec::new(),
                                admission_time_unix_ms: None,
                            },
                            DaemonRequest::RemoteWrite { .. } => DaemonResponse::WriteOk,
                            DaemonRequest::RemoteResize { .. } => DaemonResponse::ResizeOk,
                            _ => panic!("unexpected request forwarded to the legacy peer"),
                        };
                        let mut json = serde_json::to_string(&response).unwrap();
                        json.push('\n');
                        writer.write_all(json.as_bytes()).await.unwrap();
                        let _ = writer.flush().await;
                    }
                });
            }
        });

        let legacy_peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
            socket,
            vec![session_id.clone()],
        ));
        server.session_router.add_legacy_peer(legacy_peer);

        let (mut client_tx, server_rx) = tokio::io::duplex(4096);
        let s = server.clone();
        let server_handler = tokio::spawn(async move {
            s.handle_client(server_rx).await;
        });

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: None,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        client_tx.write_all(hs_json.as_bytes()).await.unwrap();
        client_tx.flush().await.unwrap();

        let (client_r, mut client_w) = tokio::io::split(client_tx);
        let mut client_lines = tokio::io::BufReader::new(client_r).lines();
        let _ = client_lines.next_line().await.unwrap();

        let write_req = DaemonRequest::RemoteWrite {
            session_id: session_id.clone(),
            generation: 1,
            data: b"hello".to_vec(),
        };
        let mut json = serde_json::to_string(&write_req).unwrap();
        json.push('\n');
        client_w.write_all(json.as_bytes()).await.unwrap();
        client_w.flush().await.unwrap();

        let resp_line = client_lines.next_line().await.unwrap().unwrap();
        let resp: DaemonResponse = serde_json::from_str(&resp_line).unwrap();
        assert!(matches!(resp, DaemonResponse::WriteOk));

        let resize_req = DaemonRequest::RemoteResize {
            session_id: session_id.clone(),
            generation: 1,
            cols: 120,
            rows: 40,
        };
        let mut json = serde_json::to_string(&resize_req).unwrap();
        json.push('\n');
        client_w.write_all(json.as_bytes()).await.unwrap();
        client_w.flush().await.unwrap();

        let resp_line = client_lines.next_line().await.unwrap().unwrap();
        let resp: DaemonResponse = serde_json::from_str(&resp_line).unwrap();
        assert!(matches!(resp, DaemonResponse::ResizeOk));

        drop(client_w);
        drop(client_lines);
        let _ = server_handler.await;
        peer_task.abort();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn regression_daemon_remote_spawn_8_retry_remote_session_preserves_structured_error() {
        let server = Arc::new(DaemonServer::new());
        let session_id = uuid::Uuid::new_v4().to_string();

        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("fx-legacy")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = dir.path().join("peer.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();

        let peer_task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (reader, mut writer) = tokio::io::split(stream);
                    let mut lines = tokio::io::BufReader::new(reader).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let response = match serde_json::from_str::<DaemonRequest>(&line).unwrap() {
                            DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                                version: DAEMON_PROTOCOL_VERSION,
                                pid: 1,
                                epoch: 1,
                                binary_path: None,
                                binary_mtime_ms: None,
                                daemon_version: None,
                                capabilities: Vec::new(),
                                admission_time_unix_ms: None,
                            },
                            DaemonRequest::RetryRemoteSession { .. } => DaemonResponse::Error {
                                message: "ssh host unreachable".to_string(),
                                code: Some("SSH_UNREACHABLE".to_string()),
                                details: None,
                            },
                            _ => panic!("unexpected request forwarded to the legacy peer"),
                        };
                        let mut json = serde_json::to_string(&response).unwrap();
                        json.push('\n');
                        writer.write_all(json.as_bytes()).await.unwrap();
                        let _ = writer.flush().await;
                    }
                });
            }
        });

        let legacy_peer = Arc::new(crate::daemon::proxy::LegacyPeer::new(
            socket,
            vec![session_id.clone()],
        ));
        server.session_router.add_legacy_peer(legacy_peer);

        let (mut client_tx, server_rx) = tokio::io::duplex(4096);
        let s = server.clone();
        let server_handler = tokio::spawn(async move {
            s.handle_client(server_rx).await;
        });

        #[cfg(not(unix))]
        let token = Some(server.transport_token.clone());
        #[cfg(unix)]
        let token = None;

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        client_tx.write_all(hs_json.as_bytes()).await.unwrap();
        client_tx.flush().await.unwrap();

        let (client_r, mut client_w) = tokio::io::split(client_tx);
        let mut client_lines = tokio::io::BufReader::new(client_r).lines();
        let _ = client_lines.next_line().await.unwrap();

        let retry_req = DaemonRequest::RetryRemoteSession {
            session_id,
        };
        let mut json = serde_json::to_string(&retry_req).unwrap();
        json.push('\n');
        client_w.write_all(json.as_bytes()).await.unwrap();
        client_w.flush().await.unwrap();

        let resp_line = client_lines.next_line().await.unwrap().unwrap();
        let resp: DaemonResponse = serde_json::from_str(&resp_line).unwrap();
        match resp {
            DaemonResponse::Error { code, .. } => {
                assert_eq!(code.as_deref(), Some("SSH_UNREACHABLE"), "RetryRemoteSession must preserve structured error code");
            }
            other => panic!("expected DaemonResponse::Error with code, got: {other:?}"),
        }

        let local_id = uuid::Uuid::new_v4().to_string();
        let missing_project = format!("ssh:{}", uuid::Uuid::new_v4());
        server.session_service.session_metadata.write().insert(
            local_id.clone(),
            crate::daemon::session_service::StoredSessionMeta {
                machine_session: None,
                client_request_id: "retry-local-validation".into(),
                workspace_id: missing_project.clone(),
                worktree: None,
                cwd: dir.path().to_owned(),
                provider_claim: None,
                spawn_fingerprint: crate::daemon::session_service::SpawnRequestFingerprint {
                    workspace_id: missing_project,
                    worktree: None,
                    cwd: None,
                    cols: 80,
                    rows: 24,
                    shell: None,
                    provider_claim: None,
                    startup: None,
                    requested_session_id: None,
                },
            },
        );
        let mut json = serde_json::to_string(&DaemonRequest::RetryRemoteSession {
            session_id: local_id,
        }).unwrap();
        json.push('\n');
        client_w.write_all(json.as_bytes()).await.unwrap();
        client_w.flush().await.unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(2), client_lines.next_line(),
        ).await.unwrap().unwrap().unwrap();
        match serde_json::from_str::<DaemonResponse>(&response).unwrap() {
            DaemonResponse::Error { code, .. } => {
                assert_eq!(code.as_deref(), Some("WORKSPACE_NOT_FOUND"));
            }
            other => panic!("expected local validation error, got: {other:?}"),
        }

        drop(client_w);
        drop(client_lines);
        let _ = server_handler.await;
        peer_task.abort();
    }

    #[tokio::test]
    async fn test_paired_recovery_outcome_preserves_typed_blocked_reason_on_wire() {
        let dir = tempfile::tempdir().unwrap();
        let server = Arc::new(DaemonServer::new_with_paths(
            Some(dir.path().join("config")),
            Some(dir.path().join("auth")),
        ));
        let hub = server.terminal_service.output_hub().clone();
        let target = crate::remote::machine_protocol::RemoteTerminalTarget {
            machine_id: "m1".into(),
            daemon_epoch: crate::scoped_contracts::Epoch(1),
            session_id: "s1".into(),
        };
        let descriptor = crate::terminal::paired_daemon::Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: crate::scoped_contracts::Epoch(1),
            target,
            after_sequence: None,
        };
        let mut proxy = crate::terminal::paired_daemon::Proxy::new(descriptor, hub.clone()).unwrap();
        proxy.recovery_failed(&crate::paired_host::client::ClientError::local("PAIRED_HOST_UNAUTHORIZED"));
        let session_id = server.terminal_service.paired().install(proxy).unwrap();

        let (client, server_writer) = tokio::io::duplex(32 * 1024);
        let (_, rx) = hub.register_session_channels(&session_id);

        let pump_server = server.clone();
        let pump_session_id = session_id.clone();
        let pump_task = tokio::spawn(async move {
            pump_server
                .pump_session_stream::<_, tokio::io::Empty>(
                    pump_session_id,
                    rx,
                    hub,
                    server_writer,
                    None,
                    None,
                )
                .await;
        });

        let mut lines = BufReader::new(client).lines();
        let frame = tokio::time::timeout(std::time::Duration::from_secs(3), lines.next_line())
            .await
            .expect("timeout reading frame")
            .expect("read error")
            .expect("eof");
        let msg: DaemonStreamMessage = serde_json::from_str(&frame).expect("valid frame");
        match msg {
            DaemonStreamMessage::RemoteStatus {
                state,
                failure,
                descriptor,
                ..
            } => {
                assert_eq!(
                    state,
                    crate::terminal::remote::RemoteConnectionState::Disconnected
                );
                let failure = failure.expect("failure must be preserved for blocked state");
                assert_eq!(
                    failure.kind,
                    crate::terminal::remote::RemoteFailureKind::Disconnected
                );
                assert_eq!(failure.message, "PAIRED_HOST_UNAUTHORIZED");
                assert!(descriptor.is_some());
            }
            other => panic!("expected RemoteStatus, got: {other:?}"),
        }
        pump_task.abort();
    }
}
