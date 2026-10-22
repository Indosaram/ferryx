//! Todo 9: a GUI facing a live owner of another data protocol only uses the control channel.

use super::*;
use crate::daemon::client::DaemonClient;
use crate::daemon::control::{
    decode_request, encode_reply, to_line, ControlFailure, InventoryReply, OP_INVENTORY,
    OP_RETIRE_WHEN_IDLE, OP_SHUTDOWN,
};
use crate::daemon::protocol::{DaemonResponse, DAEMON_PROTOCOL_VERSION};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

type Ops = Arc<std::sync::Mutex<Vec<ControlRequest>>>;

/// One accepted connection's halves, Unix socket or loopback TCP.
type Halves = (
    Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    Box<dyn tokio::io::AsyncWrite + Send + Unpin>,
);

enum Listener {
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),
    #[cfg(not(unix))]
    Tcp(tokio::net::TcpListener),
}

impl Listener {
    async fn accept(&self) -> Halves {
        match self {
            #[cfg(unix)]
            Listener::Unix(listener) => {
                let (stream, _) = listener.accept().await.unwrap();
                let (read, write) = stream.into_split();
                (Box::new(read), Box::new(write))
            }
            #[cfg(not(unix))]
            Listener::Tcp(listener) => {
                let (stream, _) = listener.accept().await.unwrap();
                let (read, write) = stream.into_split();
                (Box::new(read), Box::new(write))
            }
        }
    }
}

/// Binds a fake control endpoint inside dir the way a daemon publishes it.
async fn bind_control(dir: &Path) -> Listener {
    #[cfg(unix)]
    {
        Listener::Unix(tokio::net::UnixListener::bind(dir.join(control::CONTROL_SOCKET_FILE)).unwrap())
    }
    #[cfg(not(unix))]
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        std::fs::write(dir.join(control::CONTROL_PORT_FILE), port.to_string()).unwrap();
        std::fs::write(dir.join(control::CONTROL_TOKEN_FILE), "control-token").unwrap();
        Listener::Tcp(listener)
    }
}

/// Binds a fake data endpoint and returns the path a DaemonClient is bound to.
async fn bind_data(dir: &Path) -> (Listener, PathBuf) {
    #[cfg(unix)]
    {
        let path = dir.join("daemon.sock");
        (Listener::Unix(tokio::net::UnixListener::bind(&path).unwrap()), path)
    }
    #[cfg(not(unix))]
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let path = dir.join("daemon.port");
        std::fs::write(&path, listener.local_addr().unwrap().port().to_string()).unwrap();
        std::fs::write(dir.join("daemon.token"), "data-token").unwrap();
        (Listener::Tcp(listener), path)
    }
}

/// Serves control connections forever, recording every decoded request and answering with
/// answer(request).
fn serve_control(
    listener: Listener,
    answer: impl Fn(&ControlRequest) -> ControlReply + Send + Sync + 'static,
) -> Ops {
    let ops: Ops = Arc::default();
    let recorded = ops.clone();
    let answer = Arc::new(answer);
    tokio::spawn(async move {
        loop {
            let (read, mut write) = listener.accept().await;
            let recorded = recorded.clone();
            let answer = answer.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let request = decode_request(&line).unwrap();
                    let reply = answer(&request);
                    recorded.lock().unwrap().push(request);
                    let frame = to_line(&encode_reply(&reply));
                    if write.write_all(frame.as_bytes()).await.is_err() {
                        return;
                    }
                    let _ = write.flush().await;
                }
            });
        }
    });
    ops
}

fn standard_answer(live: u32) -> impl Fn(&ControlRequest) -> ControlReply + Send + Sync + 'static {
    move |request| match request {
        ControlRequest::Hello => ControlReply::Hello(control::HelloReply {
            daemon_version: "2099.101.1".into(),
            protocol_version: DAEMON_PROTOCOL_VERSION + 1,
            pid: 1,
            start_time: 1,
            generation_key: None,
            epoch: 1,
            ops: control::FROZEN_OPS.iter().map(|op| op.to_string()).collect(),
        }),
        ControlRequest::Inventory => ControlReply::Inventory(InventoryReply {
            live_local: live,
            live_remote: 0,
            live_paired: 0,
        }),
        ControlRequest::RetireWhenIdle => ControlReply::Live { live },
        ControlRequest::Shutdown {
            confirm_live_sessions,
        } if *confirm_live_sessions == live => ControlReply::Live { live },
        ControlRequest::Shutdown { .. } => ControlReply::Failed(ControlFailure {
            code: CODE_COUNT_CHANGED.to_string(),
            live: Some(live),
        }),
        _ => ControlReply::Failed(ControlFailure::code(control::CODE_UNSUPPORTED_OP)),
    }
}

fn op_names(ops: &Ops) -> Vec<String> {
    ops.lock().unwrap().iter().map(|op| op.op().to_string()).collect()
}

/// Captured log output of the current thread (tokio::test runs on one thread).
#[derive(Clone, Default)]
struct LogBuf(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogBuf {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuf {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

#[tokio::test]
async fn update_pending_without_control_endpoint_is_unknown_and_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let pending = enter(dir.path(), Some("2026.1013.2".into())).await;
    assert_eq!(pending.live_sessions, None, "no control endpoint: unknown, never zero");
    assert!(!pending.retire_requested);
    assert_eq!(
        pending.log_line(),
        "code=DAEMON_UPDATE_PENDING liveSessions=null retireRequested=false",
        "an unknown count is logged as a literal null, never 0"
    );
    let error = pending.into_error();
    assert_eq!(error.code, IpcErrorCode::DaemonUpdatePending);
    let details = error.details.unwrap();
    assert_eq!(details["runningVersion"], "2026.1013.2");
    assert!(details["liveSessions"].is_null());
    assert_eq!(
        restart_terminals(dir.path(), 0).await.unwrap(),
        RestartOutcome::Unavailable,
        "without control there is nothing to shut down"
    );
}

#[tokio::test]
async fn update_pending_reads_inventory_and_sends_retire_when_idle_once() {
    let dir = tempfile::tempdir().unwrap();
    let ops = serve_control(bind_control(dir.path()).await, standard_answer(3));
    let logs = LogBuf::default();
    let writer = logs.clone();
    let _subscriber = tracing::subscriber::set_default(
        tracing_subscriber::fmt().with_ansi(false).with_writer(move || writer.clone()).finish(),
    );
    let pending = enter(dir.path(), Some("v".into())).await;
    assert_eq!(pending.live_sessions, Some(3));
    assert!(pending.retire_requested);
    let line = pending.log_line();
    assert_eq!(line, "code=DAEMON_UPDATE_PENDING liveSessions=3 retireRequested=true");
    // The QA harness (q07-q09.mjs Q09) reads the count after the code on the same line.
    assert!(regex::Regex::new(r"DAEMON_UPDATE_PENDING[^\n]*\b3\b").unwrap().is_match(&line));
    assert!(!regex::Regex::new(r"DAEMON_UPDATE_PENDING[^\n]*\b4\b").unwrap().is_match(&line));
    assert_eq!(op_names(&ops), vec![OP_INVENTORY, OP_RETIRE_WHEN_IDLE]);
    let again = enter(dir.path(), Some("v".into())).await;
    assert!(again.retire_requested);
    assert_eq!(
        op_names(&ops),
        vec![OP_INVENTORY, OP_RETIRE_WHEN_IDLE, OP_INVENTORY],
        "RetireWhenIdle is sent once per GUI run; no Shutdown is ever sent automatically"
    );
    // QA Q09 counts the op name in the GUI log: exactly one line, however often pending is entered.
    let text = logs.text();
    assert_eq!(text.matches("RetireWhenIdle").count(), 1, "{text}");
}

#[tokio::test]
async fn failed_inventory_is_unknown_and_disables_restart() {
    let dir = tempfile::tempdir().unwrap();
    let ops = serve_control(bind_control(dir.path()).await, |request| match request {
        ControlRequest::RetireWhenIdle => ControlReply::Live { live: 1 },
        _ => ControlReply::Failed(ControlFailure::code(control::CODE_UNSUPPORTED_OP)),
    });
    let pending = enter(dir.path(), Some("v".into())).await;
    assert_eq!(pending.live_sessions, None);
    assert!(pending.into_error().details.unwrap()["liveSessions"].is_null());
    assert!(!op_names(&ops).iter().any(|op| op == OP_SHUTDOWN));
}

#[tokio::test]
async fn confirmed_equal_count_sends_shutdown_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let ops = serve_control(bind_control(dir.path()).await, standard_answer(2));
    assert_eq!(
        restart_terminals(dir.path(), 2).await.unwrap(),
        RestartOutcome::Accepted { live: 2 }
    );
    assert_eq!(
        *ops.lock().unwrap(),
        vec![ControlRequest::Shutdown {
            confirm_live_sessions: 2
        }]
    );
}

#[tokio::test]
async fn changed_count_answers_count_changed_and_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let ops = serve_control(bind_control(dir.path()).await, standard_answer(3));
    assert_eq!(
        restart_terminals(dir.path(), 2).await.unwrap(),
        RestartOutcome::CountChanged { live: Some(3) }
    );
    assert_eq!(op_names(&ops), vec![OP_SHUTDOWN], "one Shutdown, no automatic retry with the new count");
}

/// Serves exactly one data connection: answers the handshake with reply, then records every
/// further line until the client closes.
fn serve_one_data_connection(
    listener: Listener,
    reply: DaemonResponse,
) -> tokio::task::JoinHandle<Vec<String>> {
    tokio::spawn(async move {
        let (read, mut write) = listener.accept().await;
        let mut lines = BufReader::new(read).lines();
        let mut seen = vec![lines.next_line().await.unwrap().unwrap()];
        let frame = serde_json::to_string(&reply).unwrap() + "\n";
        write.write_all(frame.as_bytes()).await.unwrap();
        write.flush().await.unwrap();
        while let Ok(Some(line)) = lines.next_line().await {
            seen.push(line);
        }
        seen
    })
}

async fn assert_mismatch_only_reports_pending(reply: DaemonResponse, expected_ops: &[&str]) {
    let dir = tempfile::tempdir().unwrap();
    let control_ops = serve_control(bind_control(dir.path()).await, standard_answer(4));
    let (data, endpoint) = bind_data(dir.path()).await;
    let data_lines = serve_one_data_connection(data, reply);
    let client = DaemonClient::new_with_socket(endpoint.clone());
    let error = tokio::time::timeout(Duration::from_secs(5), client.list_sessions())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, IpcErrorCode::DaemonUpdatePending, "{error:?}");
    assert_eq!(error.details.as_ref().unwrap()["liveSessions"], 4);
    assert_eq!(error.details.as_ref().unwrap()["runningVersion"], "2099.101.1");
    drop(client);
    let lines = tokio::time::timeout(Duration::from_secs(5), data_lines)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lines.len(), 1, "only the handshake reaches the old owner: {lines:?}");
    assert!(lines[0].contains("\"handshake\""), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("shutdown")), "zero Shutdown");
    assert_eq!(
        op_names(&control_ops),
        expected_ops,
        "the control channel gets (Hello,) Inventory and one RetireWhenIdle, never Shutdown"
    );
    assert!(endpoint.exists(), "the live owner's endpoint file is never deleted");
    // Zero spawn and zero terminate: no successor process exists to hold the endpoint, and the
    // fake owner's listener kept serving until the client left (no kill reached it).
    assert!(!dir.path().join("starting.json").exists(), "no start was admitted over a live owner");
}

#[tokio::test]
async fn protocol_mismatch_reply_with_live_sessions_never_shuts_down_terminates_or_spawns() {
    assert_mismatch_only_reports_pending(DaemonResponse::ProtocolMismatch {
        expected_version: DAEMON_PROTOCOL_VERSION + 1,
        received_version: DAEMON_PROTOCOL_VERSION,
    }, &[UPGRADE_PROBE, &[control::OP_HELLO, OP_INVENTORY, OP_RETIRE_WHEN_IDLE]].concat())
    .await;
}

#[tokio::test]
async fn handshake_ok_of_another_version_never_shuts_down_terminates_or_spawns() {
    assert_mismatch_only_reports_pending(DaemonResponse::HandshakeOk {
        version: DAEMON_PROTOCOL_VERSION + 1,
        pid: std::process::id(),
        epoch: 9,
        binary_path: None,
        binary_mtime_ms: None,
        daemon_version: Some("2099.101.1".into()),
        capabilities: vec![control::CONTROL_CAPABILITY.to_string()],
        admission_time_unix_ms: None,
    }, &[UPGRADE_PROBE, &[OP_INVENTORY, OP_RETIRE_WHEN_IDLE]].concat())
    .await;
}

/// Control frames the client sends for its upgrade attempt before entering pending: on Unix a
/// Hello (the fake owner lists no handover-v1, so the route is Unsupported and nothing is staged
/// or requested); on Windows none, because pending is entered directly.
#[cfg(unix)]
const UPGRADE_PROBE: &[&str] = &[control::OP_HELLO];
#[cfg(not(unix))]
const UPGRADE_PROBE: &[&str] = &[];

/// A control owner that offers the todo 8 handover and answers RequestUpgrade with reply.
fn upgrade_answer(
    live: u32,
    reply: ControlReply,
) -> impl Fn(&ControlRequest) -> ControlReply + Send + Sync + 'static {
    let standard = standard_answer(live);
    move |request| match request {
        ControlRequest::Hello => ControlReply::Hello(control::HelloReply {
            daemon_version: "2099.101.1".into(),
            protocol_version: DAEMON_PROTOCOL_VERSION + 1,
            pid: 1,
            start_time: 1,
            generation_key: None,
            epoch: 1,
            ops: vec![
                control::OP_HELLO.to_string(),
                control::OP_REQUEST_UPGRADE.to_string(),
                control::HANDOVER_V1_OP.to_string(),
            ],
        }),
        ControlRequest::RequestUpgrade { .. } => reply.clone(),
        other => standard(other),
    }
}

async fn fake_stage() -> Result<crate::daemon::runtime_stage::StagedRuntime, String> {
    Ok(crate::daemon::runtime_stage::StagedRuntime {
        key: "gen-new".into(),
        executable: PathBuf::from("/staged/gen-new/ferryx"),
    })
}

/// Accepted upgrade: the owner hands over, so no pending state, no Inventory and no
/// RetireWhenIdle; a second mismatch during the handover sends nothing at all.
#[tokio::test]
async fn accepted_control_upgrade_means_no_pending_and_no_retire() {
    use crate::daemon::client::request_upgrade_via_control;
    let dir = tempfile::tempdir().unwrap();
    let ops = serve_control(
        bind_control(dir.path()).await,
        upgrade_answer(2, ControlReply::UpgradeAccepted),
    );
    let resolution =
        resolve_mismatch(dir.path(), None, Some(request_upgrade_via_control(dir.path(), fake_stage))).await;
    assert_eq!(resolution, MismatchResolution::UpgradeAccepted);
    let error = resolution.into_error();
    assert_eq!(error.code, IpcErrorCode::DaemonRetiring);
    assert_eq!(error.details.unwrap()["type"], "daemonUpgradeAccepted");
    assert_eq!(op_names(&ops), vec![control::OP_HELLO, control::OP_REQUEST_UPGRADE]);
    match &ops.lock().unwrap()[1] {
        ControlRequest::RequestUpgrade { staged_executable, generation_key } => {
            assert_eq!(generation_key.as_deref(), Some("gen-new"));
            assert!(staged_executable.ends_with("ferryx"), "{staged_executable}");
        }
        other => panic!("expected RequestUpgrade, got {other:?}"),
    }

    let again =
        resolve_mismatch(dir.path(), None, Some(request_upgrade_via_control(dir.path(), fake_stage))).await;
    assert_eq!(again, MismatchResolution::UpgradeAccepted);
    assert_eq!(op_names(&ops).len(), 2, "no second RequestUpgrade, no RetireWhenIdle, no Shutdown");
}

/// Refused upgrade: the pending state is entered (Inventory, RetireWhenIdle once), never Shutdown.
#[tokio::test]
async fn refused_control_upgrade_enters_pending_and_retires_when_idle() {
    use crate::daemon::client::request_upgrade_via_control;
    let dir = tempfile::tempdir().unwrap();
    let ops = serve_control(
        bind_control(dir.path()).await,
        upgrade_answer(3, ControlReply::Failed(ControlFailure::code("UPGRADE_REFUSED"))),
    );
    let resolution =
        resolve_mismatch(dir.path(), None, Some(request_upgrade_via_control(dir.path(), fake_stage))).await;
    let MismatchResolution::Pending(pending) = &resolution else {
        panic!("a refused upgrade must enter pending: {resolution:?}");
    };
    assert_eq!(pending.live_sessions, Some(3));
    assert!(pending.retire_requested);
    assert_eq!(pending.running_version.as_deref(), Some("2099.101.1"));
    assert_eq!(
        op_names(&ops),
        vec![
            control::OP_HELLO,
            control::OP_REQUEST_UPGRADE,
            control::OP_HELLO,
            OP_INVENTORY,
            OP_RETIRE_WHEN_IDLE
        ]
    );
    assert!(!op_names(&ops).iter().any(|op| op == OP_SHUTDOWN));
}

/// Unsupported, missing control and a failed request all fall back to pending; no upgrade future
/// (Windows) enters pending directly without any upgrade frame.
#[tokio::test]
async fn unavailable_or_failed_upgrade_and_windows_path_enter_pending() {
    use crate::daemon::client::ControlUpgradeOutcome;
    for outcome in [
        Ok(ControlUpgradeOutcome::Unsupported),
        Ok(ControlUpgradeOutcome::LegacyDaemonLeftAlone),
        Ok(ControlUpgradeOutcome::Refused("UPGRADE_UNSUPPORTED".into())),
        Err("control connect failed".to_string()),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let ops = serve_control(bind_control(dir.path()).await, standard_answer(1));
        let resolution =
            resolve_mismatch(dir.path(), Some("v".into()), Some(std::future::ready(outcome.clone()))).await;
        assert!(matches!(resolution, MismatchResolution::Pending(_)), "{outcome:?}: {resolution:?}");
        assert_eq!(op_names(&ops), vec![OP_INVENTORY, OP_RETIRE_WHEN_IDLE], "{outcome:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let ops = serve_control(bind_control(dir.path()).await, standard_answer(1));
    let no_upgrade: Option<std::future::Ready<Result<ControlUpgradeOutcome, String>>> = None;
    let resolution = resolve_mismatch(dir.path(), Some("v".into()), no_upgrade).await;
    assert!(matches!(resolution, MismatchResolution::Pending(_)));
    assert_eq!(op_names(&ops), vec![OP_INVENTORY, OP_RETIRE_WHEN_IDLE]);
}

/// Probe whose owner record names pid 77; alive toggles whether that process (and its watchdog
/// 78) still exist. The daemon lock is free and the endpoint missing, as after the old owner and
/// its watchdog exit.
struct GoneOwnerProbe {
    owner_alive: bool,
    watchdog_alive: bool,
}

impl crate::daemon::owner_admission::OwnerProbe for GoneOwnerProbe {
    fn lock_state(&self) -> crate::daemon::owner_admission::LockState {
        crate::daemon::owner_admission::LockState::Free
    }
    fn read_record(&self) -> Result<Option<crate::daemon::owner_admission::OwnerRecord>, String> {
        Ok(Some(crate::daemon::owner_admission::OwnerRecord {
            pid: 77,
            start_time: 700,
            generation_key: "old-generation".into(),
            epoch: 1,
            supervisor_pid: Some(78),
            supervisor_start_time: Some(780),
        }))
    }
    fn process_start_time(&self, pid: u32) -> Option<u64> {
        match pid {
            77 if self.owner_alive => Some(700),
            78 if self.watchdog_alive => Some(780),
            _ => None,
        }
    }
    fn endpoint(&self) -> Result<crate::daemon::owner_admission::Endpoint, String> {
        Ok(crate::daemon::owner_admission::Endpoint::Missing)
    }
    fn read_starting(&self) -> Result<Option<crate::daemon::owner_admission::StartingRecord>, String> {
        Ok(None)
    }
    fn now_unix_ms(&self) -> u64 {
        1_000_000
    }
}

struct CountingStart(std::cell::Cell<u32>);

impl crate::daemon::owner_admission::StartRunner for CountingStart {
    fn start(&self) -> Result<(), String> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}

/// The newest generation is started only once the old owner and its watchdog are gone (rows
/// 2/5c Unknown before that), and then exactly once.
#[test]
fn newest_generation_starts_once_only_after_old_owner_and_watchdog_are_gone() {
    use crate::daemon::owner_admission::{
        admit_and_start, classify_owner_with, Admission, AdmissionPaths, OwnerState,
    };
    let dir = tempfile::tempdir().unwrap();
    let paths = AdmissionPaths::for_endpoint(&dir.path().join("daemon.sock"));
    let runner = CountingStart(std::cell::Cell::new(0));

    // Old owner still running (it kept its terminals): Unknown, nothing started.
    let alive = GoneOwnerProbe { owner_alive: true, watchdog_alive: true };
    assert!(matches!(classify_owner_with(&alive, true), OwnerState::Unknown { .. }));
    // Owner exited but its Windows watchdog has not yet stopped: still Unknown (row 5c).
    let watchdog = GoneOwnerProbe { owner_alive: false, watchdog_alive: true };
    assert!(matches!(classify_owner_with(&watchdog, true), OwnerState::Unknown { .. }));
    let result = admit_and_start(&paths, &alive, &runner, "newest", Duration::ZERO, Duration::ZERO).unwrap();
    assert!(matches!(result, Admission::NotStarted(_)));
    assert_eq!(runner.0.get(), 0, "no start while the old owner lives");

    // Both gone: Absent, exactly one newest-generation start.
    let gone = GoneOwnerProbe { owner_alive: false, watchdog_alive: false };
    assert_eq!(classify_owner_with(&gone, true), OwnerState::Absent);
    let result = admit_and_start(&paths, &gone, &runner, "newest", Duration::ZERO, Duration::ZERO).unwrap();
    assert!(matches!(result, Admission::Started { .. }));
    assert_eq!(runner.0.get(), 1);
    assert!(paths.starting.exists(), "the start is recorded so concurrent callers see row 5b");
}
