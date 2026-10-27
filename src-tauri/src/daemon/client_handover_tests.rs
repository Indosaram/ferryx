//! Daemon client behaviour across a rolling handover: HANDOVER_BUSY is retried, a cached slot
//! whose peer went away is reconnected, streaming handshakes are bounded, a transient upgrade
//! failure stays retriable, and local-control classification is cached per session.
use super::*;
use tokio::net::UnixListener;

/// What the fake daemon does with one request frame.
enum Reply {
    Send(Box<DaemonResponse>),
    /// Close the connection without answering.
    Close,
    /// Keep the connection open and never answer.
    Silent,
}

type Handler = Arc<dyn Fn(usize, &DaemonRequest) -> Reply + Send + Sync>;

/// A scripted daemon on a private UDS socket. Every request frame (handshakes included) is
/// logged with the index of the connection it arrived on.
struct FakeDaemon {
    _root: tempfile::TempDir,
    socket: PathBuf,
    log: Arc<parking_lot::Mutex<Vec<(usize, DaemonRequest)>>>,
    task: tokio::task::JoinHandle<()>,
}

impl FakeDaemon {
    fn start(handler: Handler) -> Self {
        let root = tempfile::Builder::new()
            .prefix("fxh")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = root.path().join("d.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let task_log = Arc::clone(&log);
        let task = tokio::spawn(async move {
            let mut index = 0usize;
            while let Ok((stream, _)) = listener.accept().await {
                let connection = index;
                index += 1;
                let handler = Arc::clone(&handler);
                let log = Arc::clone(&task_log);
                tokio::spawn(async move {
                    let (read, mut write) = stream.into_split();
                    let mut reader = BufReader::new(read);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                        let Ok(request) = serde_json::from_str::<DaemonRequest>(line.trim()) else {
                            return;
                        };
                        let reply = handler(connection, &request);
                        log.lock().push((connection, request));
                        match reply {
                            Reply::Send(response) => {
                                let mut json = serde_json::to_string(&*response).unwrap();
                                json.push('\n');
                                if write.write_all(json.as_bytes()).await.is_err() {
                                    return;
                                }
                            }
                            Reply::Close => return,
                            Reply::Silent => {
                                // Hold both halves open until the peer goes away.
                                let mut sink = String::new();
                                let _ = reader.read_line(&mut sink).await;
                                return;
                            }
                        }
                    }
                });
            }
        });
        Self {
            _root: root,
            socket,
            log,
            task,
        }
    }

    fn client(&self) -> DaemonClient {
        DaemonClient::new_with_socket(self.socket.clone())
    }

    fn count(&self, matches: impl Fn(&DaemonRequest) -> bool) -> usize {
        self.log.lock().iter().filter(|(_, r)| matches(r)).count()
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn handshake_ok() -> DaemonResponse {
    DaemonResponse::HandshakeOk {
        version: DAEMON_PROTOCOL_VERSION,
        pid: std::process::id(),
        epoch: 1,
        binary_path: None,
        binary_mtime_ms: None,
        daemon_version: None,
        capabilities: Vec::new(),
        admission_time_unix_ms: None,
    }
}

fn local_details() -> DaemonResponse {
    DaemonResponse::RemoteSessionDetailsOk {
        details: None,
        legacy_direct_ssh: false,
    }
}

fn handover_busy() -> DaemonResponse {
    DaemonResponse::Error {
        code: Some(crate::daemon::handover::HANDOVER_BUSY.into()),
        message: "HANDOVER_BUSY: handover is committing".into(),
        details: None,
    }
}

fn is_write(request: &DaemonRequest) -> bool {
    matches!(request, DaemonRequest::Write { .. })
}

fn is_details(request: &DaemonRequest) -> bool {
    matches!(request, DaemonRequest::RemoteSessionDetails { .. })
}

#[tokio::test]
async fn handover_busy_write_is_retried_until_the_daemon_accepts_it() {
    let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler_writes = Arc::clone(&writes);
    let daemon = FakeDaemon::start(Arc::new(move |_: usize, request: &DaemonRequest| match request {
        DaemonRequest::Handshake { .. } => Reply::Send(Box::new(handshake_ok())),
        DaemonRequest::RemoteSessionDetails { .. } => Reply::Send(Box::new(local_details())),
        DaemonRequest::Write { .. } => {
            if handler_writes.fetch_add(1, Ordering::SeqCst) == 0 {
                Reply::Send(Box::new(handover_busy()))
            } else {
                Reply::Send(Box::new(DaemonResponse::WriteOk))
            }
        }
        _ => Reply::Close,
    }));
    let client = daemon.client();
    tokio::time::timeout(
        Duration::from_secs(5),
        client.write_terminal("busy-session", b"x".to_vec()),
    )
    .await
    .expect("write settles")
    .expect("a HANDOVER_BUSY refusal is retried, not reported");
    assert_eq!(daemon.count(is_write), 2);
}

#[tokio::test]
async fn persistent_handover_busy_keeps_its_code_and_reports_nothing_written() {
    let daemon = FakeDaemon::start(Arc::new(|_: usize, request: &DaemonRequest| match request {
        DaemonRequest::Handshake { .. } => Reply::Send(Box::new(handshake_ok())),
        DaemonRequest::RemoteSessionDetails { .. } => Reply::Send(Box::new(local_details())),
        DaemonRequest::Write { .. } | DaemonRequest::Resize { .. } => Reply::Send(Box::new(handover_busy())),
        _ => Reply::Close,
    }));
    let client = daemon.client();
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        client.write_terminal("busy-session", b"x".to_vec()),
    )
    .await
    .expect("write settles")
    .expect_err("a daemon that stays busy is reported");
    assert_eq!(error.code, IpcErrorCode::from_code_str("HANDOVER_BUSY"));
    let details = error.details.expect("details");
    assert_eq!(details["inputWritten"], false);
    assert_eq!(details["kind"], "busy");

    let error = tokio::time::timeout(
        Duration::from_secs(5),
        client.resize_terminal("busy-session", 80, 24),
    )
    .await
    .expect("resize settles")
    .expect_err("a daemon that stays busy is reported");
    assert_eq!(error.code, IpcErrorCode::from_code_str("HANDOVER_BUSY"));
}

#[tokio::test]
async fn a_cached_slot_whose_peer_closed_is_reconnected_and_the_write_lands_once() {
    let daemon = FakeDaemon::start(Arc::new(|_: usize, request: &DaemonRequest| match request {
        DaemonRequest::Handshake { .. } => Reply::Send(Box::new(handshake_ok())),
        DaemonRequest::RemoteSessionDetails { .. } => Reply::Send(Box::new(local_details())),
        DaemonRequest::Write { .. } => Reply::Send(Box::new(DaemonResponse::WriteOk)),
        _ => Reply::Close,
    }));
    let client = daemon.client();
    client.require_local_control("handed-over").await.expect("cache local classification before handover");
    // The slot holds a connection to a predecessor that committed and closed it.
    let (dead_client, dead_peer) = tokio::net::UnixStream::pair().unwrap();
    drop(dead_peer);
    let (reader, writer) = dead_client.into_split();
    client.local_connections.lock().insert(
        "handed-over".into(),
        Arc::new(LocalSessionSlot {
            connection: Mutex::new(Some(ActiveConnection {
                reader: BufReader::new(reader),
                writer,
            })),
            active_refs: std::sync::atomic::AtomicUsize::new(0),
            closed: AtomicBool::new(false),
        }),
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        client.write_terminal("handed-over", b"after-commit".to_vec()),
    )
    .await
    .expect("write settles")
    .expect("a dead cached slot is reconnected");
    assert_eq!(daemon.count(is_write), 1, "the write is delivered exactly once");
}

#[tokio::test]
async fn an_applied_write_without_a_reply_is_not_replayed() {
    let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let applied = Arc::clone(&writes);
    let daemon = FakeDaemon::start(Arc::new(move |_: usize, request: &DaemonRequest| match request {
        DaemonRequest::Handshake { .. } => Reply::Send(Box::new(handshake_ok())),
        DaemonRequest::RemoteSessionDetails { .. } => Reply::Send(Box::new(local_details())),
        DaemonRequest::Write { .. } => {
            if applied.fetch_add(1, Ordering::SeqCst) == 0 {
                Reply::Close
            } else {
                Reply::Send(Box::new(DaemonResponse::WriteOk))
            }
        }
        _ => Reply::Close,
    }));
    let client = daemon.client();
    client.require_local_control("lost-reply").await.expect("warm cached slot");

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        client.write_terminal("lost-reply", b"run-once\n".to_vec()),
    )
    .await
    .expect("write settles");

    assert_eq!(writes.load(Ordering::SeqCst), 1, "lost reply must not replay input");
    let error = result.expect_err("delivery without acknowledgment is ambiguous");
    assert_eq!(error.details.expect("delivery details")["type"], "ambiguousDelivery");
}

#[tokio::test]
async fn attach_to_a_peer_that_never_answers_the_handshake_fails_within_six_seconds() {
    let daemon = FakeDaemon::start(Arc::new(|_: usize, _: &DaemonRequest| Reply::Silent));
    let client = daemon.client();
    let outcome = tokio::time::timeout(Duration::from_secs(6), client.attach("s1", None)).await;
    let error = match outcome {
        Ok(Err(error)) => error,
        Ok(Ok(_)) => panic!("a silent peer cannot attach"),
        Err(_) => panic!("attach hung on a silent handshake"),
    };
    assert_ne!(error.code, IpcErrorCode::InternalError, "{error:?}");
}

#[tokio::test]
async fn subscriptions_to_a_peer_that_never_answers_fail_within_six_seconds() {
    let daemon = FakeDaemon::start(Arc::new(|_: usize, _: &DaemonRequest| Reply::Silent));
    let client = daemon.client();
    let events = tokio::time::timeout(Duration::from_secs(6), client.subscribe_remote_events()).await;
    assert!(matches!(events, Ok(Err(_))), "remote events hung or succeeded");
    let dag = tokio::time::timeout(Duration::from_secs(6), client.subscribe_dag("ws", "/p")).await;
    assert!(matches!(dag, Ok(Err(_))), "dag subscription hung or succeeded");
}

#[tokio::test]
async fn a_list_sessions_failure_leaves_the_upgrade_retriable() {
    let daemon = FakeDaemon::start(Arc::new(|_: usize, request: &DaemonRequest| match request {
        DaemonRequest::Handshake { .. } => Reply::Send(Box::new(handshake_ok())),
        DaemonRequest::ListSessions => Reply::Send(Box::new(DaemonResponse::Error {
            code: None,
            message: "transient".into(),
            details: None,
        })),
        _ => Reply::Close,
    }));
    let client = daemon.client();
    assert!(client.admit_upgrade_attempt());
    let control_dir = daemon.socket.parent().unwrap().to_path_buf();
    tokio::time::timeout(
        Duration::from_secs(5),
        DaemonClient::run_upgrade_attempt(
            client.upgrade_rpc_client(),
            client.clone(),
            control_dir,
            Some("2026.1.1".into()),
            None,
            None,
            || async { Err::<crate::daemon::runtime_stage::StagedRuntime, String>("not staged".into()) },
        ),
    )
    .await
    .expect("upgrade attempt settles");
    assert!(
        !client.upgrade_requested.load(Ordering::SeqCst),
        "a transient failure must not latch the upgrade for the rest of the run"
    );
}

#[tokio::test]
async fn a_second_local_write_on_the_same_session_sends_no_classification() {
    let daemon = FakeDaemon::start(Arc::new(|_: usize, request: &DaemonRequest| match request {
        DaemonRequest::Handshake { .. } => Reply::Send(Box::new(handshake_ok())),
        DaemonRequest::RemoteSessionDetails { .. } => Reply::Send(Box::new(local_details())),
        DaemonRequest::Write { .. } => Reply::Send(Box::new(DaemonResponse::WriteOk)),
        _ => Reply::Close,
    }));
    let client = daemon.client();
    for chunk in [b"a".to_vec(), b"b".to_vec()] {
        tokio::time::timeout(Duration::from_secs(5), client.write_terminal("local", chunk))
            .await
            .expect("write settles")
            .expect("local write");
    }
    assert_eq!(daemon.count(is_write), 2);
    assert_eq!(daemon.count(is_details), 1, "the local classification is cached");
}
