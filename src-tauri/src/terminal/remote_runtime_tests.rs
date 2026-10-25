use super::output_hub::TerminalOutputHub;
use super::remote::*;
use crate::{scoped_contracts::TargetRef, ssh::bridge::*};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch, Mutex, Semaphore};
type Rpc<'a, T> = Pin<Box<dyn Future<Output = Result<T, BridgeError>> + Send + 'a>>;
struct Fake {
    reads: Mutex<mpsc::UnboundedReceiver<Result<ReadResult, BridgeError>>>,
    describes: AtomicUsize,
    writes: AtomicUsize,
    stops: AtomicUsize,
    write_failure: parking_lot::Mutex<Option<BridgeError>>,
    write_entered: parking_lot::Mutex<Option<Arc<tokio::sync::Notify>>>,
    write_pause: parking_lot::Mutex<Option<Arc<tokio::sync::Notify>>>,
    /// When set, `stop` signals `.0` on entry and waits for `.1` before completing.
    stop_pause: parking_lot::Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    /// When true, `describe` reports the target as exited.
    describe_exited: std::sync::atomic::AtomicBool,
    write_history: parking_lot::Mutex<Vec<Vec<u8>>>,
    resizes: AtomicUsize,
    last_resize: parking_lot::Mutex<Option<(u16, u16)>>,
    supports_agent_state: bool,
    agent_acks: parking_lot::Mutex<Vec<RemoteCursor>>,
    /// Bumped inside `Transport::read` for every read the pump issues, before that read's
    /// response can be consumed. Tests await it to order assertions against the pump's handling
    /// of the previous response instead of polling the transport channel.
    read_calls: watch::Sender<usize>,
}
impl Transport for Fake {
    fn describe<'a>(&'a self, t: &'a TargetRef) -> Rpc<'a, DescribeResult> {
        Box::pin(async move {
            self.describes.fetch_add(1, Ordering::SeqCst);
            Ok(DescribeResult {
                target: t.clone(),
                pid: RemotePid(999999),
                cwd: "/project/wt".into(),
                cols: 80,
                rows: 24,
                cursor: RemoteCursor(900),
                exited: self.describe_exited.load(Ordering::SeqCst),
            })
        })
    }
    fn read<'a>(
        &'a self,
        _: &'a TargetRef,
        _: RemoteCursor,
        agent_after_revision: RemoteCursor,
    ) -> Rpc<'a, ReadResult> {
        self.agent_acks.lock().push(agent_after_revision);
        let issued = *self.read_calls.borrow() + 1;
        self.read_calls.send_replace(issued);
        Box::pin(async move {
            self.reads
                .lock()
                .await
                .recv()
                .await
                .expect("test retains sender")
        })
    }
    fn write<'a>(&'a self, _: &'a TargetRef, bytes: &'a [u8]) -> Rpc<'a, ()> {
        let entered = self.write_entered.lock().take();
        let pause = self.write_pause.lock().take();
        let payload = bytes.to_vec();
        Box::pin(async move {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.write_history.lock().push(payload);
            if let Some(entered) = entered {
                entered.notify_one();
            }
            if let Some(pause) = pause {
                pause.notified().await;
            }
            if let Some(error) = self.write_failure.lock().take() {
                return Err(error);
            }
            Ok(())
        })
    }
    fn resize<'a>(&'a self, _: &'a TargetRef, cols: u16, rows: u16) -> Rpc<'a, ()> {
        Box::pin(async move {
            self.resizes.fetch_add(1, Ordering::SeqCst);
            *self.last_resize.lock() = Some((cols, rows));
            Ok(())
        })
    }
    fn stop<'a>(&'a self, _: &'a TargetRef) -> Rpc<'a, ()> {
        let pause = self.stop_pause.lock().take();
        Box::pin(async move {
            if let Some((entered, release)) = pause {
                entered.notify_one();
                release.notified().await;
            }
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
    fn supports_agent_state(&self) -> bool {
        self.supports_agent_state
    }
}
struct Dialer {
    fake: Arc<Fake>,
    calls: AtomicUsize,
    clock: Arc<Semaphore>,
    failure: parking_lot::Mutex<Option<BridgeError>>,
}
impl Connector for Dialer {
    fn connect<'a>(&'a self, _: &'a RemoteSessionDescriptor) -> Rpc<'a, Arc<dyn Transport>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.failure.lock().take() {
                return Err(error);
            }
            Ok(self.fake.clone() as Arc<dyn Transport>)
        })
    }
    fn delay(&self, _: u32) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let c = self.clock.clone();
        Box::pin(async move {
            c.acquire().await.unwrap().forget();
        })
    }
}
fn descriptor() -> RemoteSessionDescriptor {
    serde_json::from_value(serde_json::json!({
    "backendSessionId":"local-stable", "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"remote-original"},
    "config":{"host":{"id":"host","label":"host","hostname":"example.invalid","source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/home/test","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"project","projectPath":"/project","worktree":"wt","agentIdentity":{"agent":"claude","id":"agent-original"}},
    "clientRequestId":"request-original","remoteCursor":"0","cols":80,"rows":24
})).unwrap()
}
fn fixture() -> (
    RemoteRuntime,
    Arc<TerminalOutputHub>,
    Arc<Dialer>,
    mpsc::UnboundedSender<Result<ReadResult, BridgeError>>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let fake = Arc::new(Fake {
        reads: Mutex::new(rx),
        describes: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        write_failure: parking_lot::Mutex::new(None),
        write_entered: parking_lot::Mutex::new(None),
        write_pause: parking_lot::Mutex::new(None),
        stop_pause: parking_lot::Mutex::new(None),
        describe_exited: std::sync::atomic::AtomicBool::new(false),
        write_history: parking_lot::Mutex::new(Vec::new()),
        resizes: AtomicUsize::new(0),
        last_resize: parking_lot::Mutex::new(None),
        supports_agent_state: false,
        agent_acks: parking_lot::Mutex::new(Vec::new()),
        read_calls: watch::channel(0usize).0,
    });
    let dialer = Arc::new(Dialer {
        fake,
        calls: AtomicUsize::new(0),
        clock: Arc::new(Semaphore::new(0)),
        failure: parking_lot::Mutex::new(None),
    });
    let hub = Arc::new(TerminalOutputHub::default());
    (
        RemoteRuntime::with_connector(hub.clone(), dialer.clone()),
        hub,
        dialer,
        tx,
    )
}
async fn state(
    rx: &mut watch::Receiver<RemoteSessionDetails>,
    predicate: impl Fn(&RemoteSessionDetails) -> bool,
) -> RemoteSessionDetails {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let d = rx.borrow_and_update().clone();
            if predicate(&d) {
                return d;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("state deadline")
}
fn output(cursor: u64, gap: bool) -> ReadResult {
    ReadResult {
        target: descriptor().target,
        pid: RemotePid(999999),
        cwd: "/project/wt".into(),
        cursor: RemoteCursor(cursor),
        after_sequence: cursor,
        gap,
        exited: false,
        chunks: vec![ReadChunk {
            cursor: RemoteCursor(cursor),
            sequence: cursor,
            data_base64: String::new(),
            bytes: format!("record-{cursor};").into_bytes(),
        }],
        agent_state: None,
    }
}

#[tokio::test]
async fn ssh_pane_resize_during_outage_is_applied_after_reconnect() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    // Force a transport outage; the redial parks on the delay clock.
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    let reconnecting = state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;
    // A resize still carrying the pre-outage generation is stale: admission must
    // reject it without remembering its geometry (covered by the dedicated stale
    // regression below). The pane then re-issues at the live generation while the
    // remote is dialing: that same-generation Disconnected outage must be
    // remembered, not dropped, so the remote PTY never stays at spawn defaults.
    assert!(runtime
        .resize("local-stable", connected.generation, 132, 43)
        .is_err());
    assert!(matches!(
        runtime
            .resize("local-stable", reconnecting.generation, 132, 43)
            .err()
            .map(|failure| failure.kind),
        Some(RemoteFailureKind::Disconnected)
    ));
    dialer.clock.add_permits(1);
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.generation > connected.generation
    })
    .await;
    let detail = state(&mut rx, |d| {
        d.descriptor.cols == 132 && d.descriptor.rows == 43
    })
    .await;
    assert_eq!(*dialer.fake.last_resize.lock(), Some((132, 43)));
    assert_eq!(detail.descriptor.target, descriptor().target);
}

#[tokio::test]
async fn ssh_pane_sync_admission_stale_generation_resize_is_not_replayed_after_reconnect() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    // Force a transport outage; the redial parks on the delay clock.
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;
    // A resize admitted on the synchronous path with a pre-outage generation was
    // rejected as StaleGeneration: its geometry must never seed pending_size,
    // otherwise the reconnect replays dimensions the newer generation never chose
    // (the queue-worker dispatch path already enforces the same rule).
    assert!(runtime
        .resize("local-stable", connected.generation, 150, 50)
        .is_err());
    dialer.clock.add_permits(1);
    let reconnected = state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.generation > connected.generation
    })
    .await;
    assert_eq!(
        *dialer.fake.last_resize.lock(),
        Some((80, 24)),
        "stale-generation geometry must not be replayed onto the new connection"
    );
    assert_eq!(
        (reconnected.descriptor.cols, reconnected.descriptor.rows),
        (80, 24)
    );
}

#[tokio::test]
async fn ssh_reconnect_reasserts_recorded_pane_size() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    runtime
        .resize("local-stable", connected.generation, 120, 40)
        .unwrap()
        .await
        .unwrap();
    assert_eq!(*dialer.fake.last_resize.lock(), Some((120, 40)));
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;
    dialer.clock.add_permits(1);
    let reconnected = state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.generation > connected.generation
    })
    .await;
    // The connect path re-asserted the last accepted size without a new request:
    // one resize at the first connect, one explicit, one at the redial.
    assert_eq!(*dialer.fake.last_resize.lock(), Some((120, 40)));
    assert_eq!(dialer.fake.resizes.load(Ordering::SeqCst), 3);
    assert_eq!(
        (reconnected.descriptor.cols, reconnected.descriptor.rows),
        (120, 40)
    );
}

#[tokio::test]
async fn ssh_reconnect_safety_control_failure_interrupts_pending_read() {
    let (runtime, _, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    *dialer.fake.write_failure.lock() = Some(BridgeError::ConnectionClosed);
    let error = runtime
        .write("local-stable", connected.generation, b"once".to_vec())
        .unwrap()
        .await
        .unwrap_err();
    assert_eq!(error.kind, RemoteFailureKind::Transport);
    state(&mut rx, |d| {
        d.attempts == 1 && d.generation > connected.generation
    })
    .await;
    dialer.clock.add_permits(1);
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.generation > connected.generation
    })
    .await;
    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 1);
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ssh_daemon_restart_rejects_duplicate_target_controller() {
    let (runtime, _, _, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut duplicate = descriptor();
    duplicate.backend_session_id = "second-local-id".into();
    assert_eq!(
        runtime.restore(duplicate).unwrap_err().kind,
        RemoteFailureKind::Protocol
    );
    assert_eq!(runtime.list(), vec!["local-stable"]);
}

#[test]
fn ssh_reconnect_safety_failure_classification() {
    assert_eq!(
        RemoteFailure::from_bridge(&BridgeError::TargetNotFound).kind,
        RemoteFailureKind::Missing
    );
    assert_eq!(
        RemoteFailure::from_bridge(&BridgeError::RemoteTargetExpired).kind,
        RemoteFailureKind::Expired
    );
    assert_eq!(
        RemoteFailure::from_bridge(&BridgeError::ProcessExited {
            code: Some(255),
            stderr: "Permission denied (publickey).".into()
        })
        .kind,
        RemoteFailureKind::Authentication
    );
    assert_eq!(
        RemoteFailure::from_bridge(&BridgeError::ConnectionClosed).kind,
        RemoteFailureKind::Transport
    );
}
#[test]
fn ssh_reconnect_safety_erased_ipc_errors_are_not_classified_by_prose() {
    for message in [
        "Permission denied",
        "authentication",
        "connection refused",
        "connection timed out",
        "no route to host",
        "connection reset",
    ] {
        assert_eq!(
            RemoteFailure::from_bridge(&BridgeError::SshPlan(message.into())).kind,
            RemoteFailureKind::Protocol,
            "An erased IPC error must not acquire a classification from its message"
        );
    }
}

#[tokio::test]
async fn ssh_reconnect_safety_setup_classifies_structured_transport_and_authentication() {
    use crate::ipc::{IpcError, IpcErrorCode};
    let cases = [
        (
            IpcErrorCode::IoError,
            serde_json::json!({"stage": "transport"}),
            RemoteFailureKind::Transport,
        ),
        (
            IpcErrorCode::IoError,
            serde_json::json!({"stage": "execution", "exitCode": 255, "stderr": "Permission denied (publickey)"}),
            RemoteFailureKind::Authentication,
        ),
        (
            IpcErrorCode::IoError,
            serde_json::json!({"stage": "execution", "exitCode": 255, "stderr": "Connection refused"}),
            RemoteFailureKind::Transport,
        ),
        (
            IpcErrorCode::InvalidArgument,
            serde_json::json!({"stage": "transport"}),
            RemoteFailureKind::Protocol,
        ),
        (
            IpcErrorCode::IoError,
            serde_json::json!({"stage": "startup"}),
            RemoteFailureKind::Protocol,
        ),
        (
            IpcErrorCode::CliExecutableNotFound,
            serde_json::json!({"stage": "helper_missing"}),
            RemoteFailureKind::Missing,
        ),
    ];
    for (code, details, expected) in cases {
        let error = IpcError::new(code, "authentication connection refused").with_details(details);
        assert_eq!(
            RemoteFailure::from_bridge(&BridgeError::from(error)).kind,
            expected
        );
    }
}

#[tokio::test]
async fn ssh_process_survival_same_target_replay_and_close() {
    let (runtime, hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    let mut live = hub.subscribe_with_sequence("local-stable", None).unwrap();
    tx.send(Ok(output(1, false))).unwrap();
    assert_eq!(
        &*tokio::time::timeout(Duration::from_secs(3), live.receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .bytes,
        b"record-1;"
    );
    // Hold the dispatch gate from before admission so the queued write cannot race
    // the outage: under eager FIFO dispatch the worker must never send this op, and
    // the gate makes that ordering deterministic rather than poll-order dependent.
    let gate = runtime.hold_dispatch_gate_for_test("local-stable").unwrap();
    let pending = runtime
        .write(
            "local-stable",
            connected.generation,
            b"must-not-send".to_vec(),
        )
        .unwrap();
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;
    assert!(runtime
        .write("local-stable", connected.generation, b"outage".to_vec())
        .is_err());
    drop(gate);
    assert_eq!(
        pending.await.unwrap_err().kind,
        RemoteFailureKind::StaleGeneration
    );
    runtime.retry("local-stable").unwrap();
    runtime.retry("local-stable").unwrap();
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 1);
    dialer.clock.add_permits(1);
    let reconnected = state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.generation > connected.generation
    })
    .await;
    tx.send(Ok(output(1, false))).unwrap();
    tx.send(Ok(output(8, true))).unwrap();
    let recovered = state(&mut rx, |d| d.descriptor.remote_cursor == RemoteCursor(8)).await;
    assert_eq!(hub.subscribe("local-stable").unwrap().0, b"record-8;");
    let boundary = live.receiver.try_recv().unwrap();
    assert!(boundary.bytes.is_empty());
    assert!(boundary.replay_gap.is_some());
    assert_eq!(&*live.receiver.try_recv().unwrap().bytes, b"record-8;");
    assert_eq!(
        recovered.replay_gap.unwrap().available_from_cursor,
        RemoteCursor(8)
    );
    assert_eq!(recovered.descriptor.target, descriptor().target);
    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 0);
    runtime
        .write("local-stable", reconnected.generation, b"new".to_vec())
        .unwrap()
        .await
        .unwrap();
    runtime.close("local-stable").await.unwrap();
    assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 1);
    assert!(!hub.has_session("local-stable"));
}
#[tokio::test]
async fn ssh_daemon_restart_descriptor_only_and_drop_does_not_stop() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Ok(output(4, false))).unwrap();
    let d = state(&mut rx, |d| d.descriptor.remote_cursor == RemoteCursor(4))
        .await
        .descriptor;
    let persisted = serde_json::to_vec(&d).unwrap();
    drop(runtime);
    assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 0);
    let (restored, _, next, _sender) = fixture();
    restored
        .restore(serde_json::from_slice(&persisted).unwrap())
        .unwrap();
    let mut rx = restored.subscribe("local-stable").unwrap();
    let detail = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    assert_eq!(detail.descriptor, d);
    assert_eq!(next.fake.describes.load(Ordering::SeqCst), 1);
    assert_eq!(
        detail.descriptor.config.agent_identity,
        descriptor().config.agent_identity
    );
}
#[tokio::test]
async fn ssh_reconnect_safety_outage_never_parks_and_terminal_failure() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    // A long outage: every redial fails well past the old five-attempt budget.
    for attempt in 1..=12u32 {
        state(&mut rx, |d| {
            d.state == RemoteConnectionState::Reconnecting && d.attempts == attempt
        })
        .await;
        *dialer.failure.lock() = Some(BridgeError::ConnectionClosed);
        dialer.clock.add_permits(1);
    }
    // The host comes back: the loop must still be dialing and reconnect on its own.
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.attempts == 13
    })
    .await;
    dialer.clock.add_permits(1);
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 14);
    tx.send(Err(BridgeError::RemoteTargetExpired)).unwrap();
    let expired = state(&mut rx, |d| d.state == RemoteConnectionState::Expired).await;
    assert_eq!(expired.failure.unwrap().kind, RemoteFailureKind::Expired);
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 14);
}

#[test]
fn ssh_reconnect_backoff_is_bounded() {
    use crate::terminal::remote::reconnect_backoff;
    assert_eq!(reconnect_backoff(0), Duration::from_millis(250));
    assert_eq!(reconnect_backoff(1), Duration::from_millis(500));
    assert_eq!(reconnect_backoff(7), Duration::from_secs(32));
    assert_eq!(reconnect_backoff(u32::MAX), Duration::from_secs(32));
}

#[tokio::test]
async fn ssh_reconnect_unreachable_target_exhausts_budget_and_settles() {
    let (runtime, hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
        state(&mut rx, |d| d.state == RemoteConnectionState::Reconnecting && d.attempts == attempt).await;
        assert!(runtime.keeps_predecessor_alive("local-stable"));
        *dialer.failure.lock() = Some(BridgeError::ConnectionClosed);
        dialer.clock.add_permits(1);
    }
    let ended = state(&mut rx, |d| d.state == RemoteConnectionState::Expired).await;
    assert!(runtime.keeps_predecessor_alive("local-stable"));
    assert_eq!(ended.attempts, MAX_RECONNECT_ATTEMPTS);
    assert!(ended.failure.unwrap().message.contains("reconnect attempts"));
    assert!(runtime.retry("local-stable").is_err());
    assert!(runtime.settle_ended("local-stable"));
    assert!(!runtime.keeps_predecessor_alive("local-stable"));
    assert!(!runtime.contains("local-stable"));
    assert!(!hub.has_session("local-stable"));
    assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ssh_reconnect_safety_authentication_on_redial_stops_retries() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.attempts == 1
    })
    .await;
    *dialer.failure.lock() = Some(BridgeError::ProcessExited {
        code: Some(255),
        stderr: "Permission denied (publickey)".into(),
    });
    dialer.clock.add_permits(1);
    let failed = state(&mut rx, |d| d.state == RemoteConnectionState::Disconnected).await;
    assert_eq!(
        failed.failure.unwrap().kind,
        RemoteFailureKind::Authentication
    );
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ssh_reconnect_safety_retry_budget_resets_after_successful_read() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.attempts == 0
    })
    .await;

    // Recover more independent outages than the consecutive-failure limit.
    for cycle in 1..=6u64 {
        tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
        state(&mut rx, |d| {
            d.state == RemoteConnectionState::Reconnecting && d.attempts == 1
        })
        .await;

        dialer.clock.add_permits(1);

        // Describe succeeds: state is Connected, but read has not succeeded yet,
        // so attempts must still be 1 (cap test invariant preserved).
        state(&mut rx, |d| {
            d.state == RemoteConnectionState::Connected && d.attempts == 1
        })
        .await;

        // Alternate output and empty long-poll responses. Both prove recovery.
        let mut read = output(cycle, false);
        if cycle % 2 == 0 {
            read.chunks.clear();
            read.cursor = RemoteCursor(cycle - 1);
            read.after_sequence = cycle - 1;
        }
        let expected_cursor = read.cursor;
        tx.send(Ok(read)).unwrap();

        // Valid read must reset attempts to 0
        let detail = state(&mut rx, |d| {
            d.state == RemoteConnectionState::Connected
                && d.attempts == 0
                && d.descriptor.remote_cursor == expected_cursor
        })
        .await;
        assert_eq!(detail.attempts, 0);
    }
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 7);
}

#[tokio::test]
async fn ssh_remote_input_concurrent_writes_are_ordered_without_busy_drop() {
    let (runtime, _, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_entered.lock() = Some(entered.clone());
    let pause = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_pause.lock() = Some(pause.clone());

    let op1 = runtime
        .write("local-stable", connected.generation, b"first".to_vec())
        .expect("op1 admitted");

    let op1_task = tokio::spawn(op1);

    entered.notified().await;

    // Under current remote.rs, op2 fails at admission because e.control.try_lock_owned() fails with Busy!
    let op2 = runtime
        .write("local-stable", connected.generation, b"second".to_vec())
        .expect("op2 must be admitted into bounded queue without Busy error");

    let op2_task = tokio::spawn(op2);

    pause.notify_one();

    let (res1, res2) = tokio::join!(op1_task, op2_task);
    res1.unwrap().unwrap();
    res2.unwrap().unwrap();

    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 2);
    let history = dialer.fake.write_history.lock().clone();
    assert_eq!(history, vec![b"first".to_vec(), b"second".to_vec()]);
}

#[tokio::test]
async fn ssh_remote_input_queued_operation_rejected_on_generation_bump() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_entered.lock() = Some(entered.clone());
    let pause = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_pause.lock() = Some(pause.clone());

    let op1 = runtime
        .write("local-stable", connected.generation, b"first".to_vec())
        .expect("op1 admitted");
    let op1_task = tokio::spawn(op1);

    entered.notified().await;

    let op2 = runtime
        .write(
            "local-stable",
            connected.generation,
            b"stale-queued".to_vec(),
        )
        .expect("op2 admitted into queue");
    let op2_task = tokio::spawn(op2);

    // Controlled failure on op1 to trigger reconnect without deadlocking on e.control
    *dialer.fake.write_failure.lock() = Some(BridgeError::ConnectionClosed);
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();

    // Release op1 so it finishes with failure and releases e.control
    pause.notify_one();
    let err1 = op1_task.await.unwrap().unwrap_err();
    assert_eq!(err1.kind, RemoteFailureKind::Transport);

    // Now run() acquires e.control cleanly and advances generation
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;

    // op2 was queued. It must safely not send to bridge, failing with Disconnected, StaleGeneration, or Transport
    let op2_res = op2_task.await.unwrap();
    let err2 = op2_res.expect_err("queued operation must fail safely under failure/bump");
    assert!(
        matches!(
            err2.kind,
            RemoteFailureKind::StaleGeneration
                | RemoteFailureKind::Disconnected
                | RemoteFailureKind::Transport
        ),
        "unexpected error kind: {:?}",
        err2.kind
    );

    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ssh_remote_input_cancelled_middle_operation_skipped_without_bypass_or_stall() {
    let (runtime, _, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_entered.lock() = Some(entered.clone());
    let pause = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_pause.lock() = Some(pause.clone());

    let op1 = runtime
        .write("local-stable", connected.generation, b"first".to_vec())
        .expect("op1 admitted");
    let op1_task = tokio::spawn(op1);

    entered.notified().await;

    let op2 = runtime
        .write(
            "local-stable",
            connected.generation,
            b"cancelled-middle".to_vec(),
        )
        .expect("op2 admitted");
    let op2_task = tokio::spawn(op2);

    let op3 = runtime
        .write("local-stable", connected.generation, b"third".to_vec())
        .expect("op3 admitted");
    let op3_task = tokio::spawn(op3);

    // Cancel op2 by aborting its task (which drops the oneshot receiver)
    op2_task.abort();
    let _ = op2_task.await;

    // Release op1 so queue proceeds
    pause.notify_one();

    op1_task.await.unwrap().unwrap();
    op3_task.await.unwrap().unwrap();

    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 2);
    let history = dialer.fake.write_history.lock().clone();
    assert_eq!(history, vec![b"first".to_vec(), b"third".to_vec()]);
}

#[tokio::test]
async fn ssh_remote_input_bounded_queue_rejects_overflow() {
    let (runtime, _, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let entered = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_entered.lock() = Some(entered.clone());
    let pause = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_pause.lock() = Some(pause.clone());

    let op1 = runtime
        .write("local-stable", connected.generation, b"in-flight".to_vec())
        .unwrap();
    let op1_task = tokio::spawn(op1);

    entered.notified().await;

    const EXPECTED_QUEUE_CAPACITY: usize = 16;
    let mut queued = Vec::new();
    for i in 0..EXPECTED_QUEUE_CAPACITY {
        let op = runtime
            .write(
                "local-stable",
                connected.generation,
                format!("q-{i}").into_bytes(),
            )
            .unwrap_or_else(|e| panic!("queued request {i} failed unexpectedly: {e:?}"));
        queued.push(tokio::spawn(op));
    }
    assert_eq!(queued.len(), EXPECTED_QUEUE_CAPACITY);

    // The (EXPECTED_QUEUE_CAPACITY + 1)th request must be rejected with Busy
    let overflow_err = match runtime.write(
        "local-stable",
        connected.generation,
        b"overflow-payload".to_vec(),
    ) {
        Ok(_) => panic!("queue bound must reject excess with Busy, but write was accepted"),
        Err(e) => e,
    };
    assert_eq!(overflow_err.kind, RemoteFailureKind::Busy);

    pause.notify_one();
    op1_task.await.unwrap().unwrap();
    for task in queued {
        task.await.unwrap().unwrap();
    }
    // Coalesced queued writes produce at most 2 transport writes for 16 items.
    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn ssh_remote_input_expired_queue_never_dispatches_delayed_keys() {
    let (runtime, _, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_entered.lock() = Some(entered.clone());
    *dialer.fake.write_pause.lock() = Some(release.clone());
    let first = runtime
        .write("local-stable", connected.generation, b"first".to_vec())
        .unwrap();
    let first = tokio::spawn(first);
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .unwrap();
    let late = runtime
        .write("local-stable", connected.generation, b"too-late".to_vec())
        .unwrap();
    tokio::time::advance(Duration::from_secs(6)).await;
    release.notify_one();
    first.await.unwrap().unwrap();
    assert!(late.await.is_err(), "expired input must not be dispatched");
    assert_eq!(*dialer.fake.write_history.lock(), vec![b"first".to_vec()]);
}

#[tokio::test]
async fn ssh_remote_input_stale_generation_resize_is_not_replayed_onto_new_connection() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let gate = runtime.hold_dispatch_gate_for_test("local-stable").unwrap();
    let resize = runtime
        .resize("local-stable", connected.generation, 100, 30)
        .unwrap();
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;
    drop(gate);
    assert!(
        resize.await.is_err(),
        "stale-generation resize must be rejected"
    );

    runtime.retry("local-stable").unwrap();
    dialer.clock.add_permits(1);
    let reconnected = state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.generation > connected.generation
    })
    .await;
    assert_eq!(reconnected.generation, connected.generation + 1);
    let last = dialer
        .fake
        .last_resize
        .lock()
        .clone()
        .expect("dial must apply descriptor geometry");
    assert_eq!(
        last,
        (80, 24),
        "stale-generation geometry must not be replayed onto the new connection"
    );
}

#[tokio::test]
async fn ssh_remote_input_coalesces_queued_writes_into_bounded_transport_batches() {
    let (runtime, _, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    // Hold dispatch gate so the 4 back-to-back writes enqueue before dispatch begins.
    let gate = runtime.hold_dispatch_gate_for_test("local-stable").unwrap();

    let op1 = runtime
        .write("local-stable", connected.generation, b"key-1;".to_vec())
        .expect("op1 admitted");
    let op2 = runtime
        .write("local-stable", connected.generation, b"key-2;".to_vec())
        .expect("op2 admitted");
    let op3 = runtime
        .write("local-stable", connected.generation, b"key-3;".to_vec())
        .expect("op3 admitted");
    let op4 = runtime
        .write("local-stable", connected.generation, b"key-4;".to_vec())
        .expect("op4 admitted");

    let t1 = tokio::spawn(op1);
    let t2 = tokio::spawn(op2);
    let t3 = tokio::spawn(op3);
    let t4 = tokio::spawn(op4);

    drop(gate);

    t1.await.unwrap().unwrap();
    t2.await.unwrap().unwrap();
    t3.await.unwrap().unwrap();
    t4.await.unwrap().unwrap();

    let write_calls = dialer.fake.writes.load(Ordering::SeqCst);
    assert!(
        write_calls <= 2,
        "4 back-to-back writes must produce at most 2 transport writes, got {write_calls}"
    );

    let history = dialer.fake.write_history.lock().clone();
    let combined_bytes: Vec<u8> = history.into_iter().flatten().collect();
    assert_eq!(
        combined_bytes,
        b"key-1;key-2;key-3;key-4;",
        "concatenated bytes must match FIFO input order"
    );
}

struct TestSink {
    published: parking_lot::Mutex<Vec<crate::daemon::agent_state::AgentState>>,
    notify: Arc<tokio::sync::Notify>,
    accept_updates: bool,
}

impl TestSink {
    fn new() -> (Arc<Self>, Arc<tokio::sync::Notify>) {
        let notify = Arc::new(tokio::sync::Notify::new());
        (
            Arc::new(Self {
                published: parking_lot::Mutex::new(Vec::new()),
                notify: notify.clone(),
                accept_updates: true,
            }),
            notify,
        )
    }
}

impl AgentStateSink for TestSink {
    fn accept(&self, state: crate::daemon::agent_state::AgentState) -> bool {
        if !self.accept_updates {
            return false;
        }
        let mut list = self.published.lock();
        if let Some(prev) = list.last() {
            if prev == &state {
                return true;
            }
        }
        list.push(state);
        self.notify.notify_one();
        true
    }
}

fn agent_fixture(supports_agent_state: bool) -> (
    RemoteRuntime,
    Arc<TerminalOutputHub>,
    Arc<Dialer>,
    mpsc::UnboundedSender<Result<ReadResult, BridgeError>>,
    Arc<TestSink>,
    Arc<tokio::sync::Notify>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let fake = Arc::new(Fake {
        reads: Mutex::new(rx),
        describes: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        write_failure: parking_lot::Mutex::new(None),
        write_entered: parking_lot::Mutex::new(None),
        write_pause: parking_lot::Mutex::new(None),
        stop_pause: parking_lot::Mutex::new(None),
        describe_exited: std::sync::atomic::AtomicBool::new(false),
        write_history: parking_lot::Mutex::new(Vec::new()),
        resizes: AtomicUsize::new(0),
        last_resize: parking_lot::Mutex::new(None),
        supports_agent_state,
        agent_acks: parking_lot::Mutex::new(Vec::new()),
        read_calls: watch::channel(0usize).0,
    });
    let dialer = Arc::new(Dialer {
        fake,
        calls: AtomicUsize::new(0),
        clock: Arc::new(Semaphore::new(0)),
        failure: parking_lot::Mutex::new(None),
    });
    let hub = Arc::new(TerminalOutputHub::default());
    let (sink, notify) = TestSink::new();
    let runtime = RemoteRuntime::with_connector_and_sink(hub.clone(), dialer.clone(), sink.clone());
    (runtime, hub, dialer, tx, sink, notify)
}

fn agent_output(cursor: u64, snapshot: Option<AgentStateSnapshot>) -> ReadResult {
    ReadResult {
        target: descriptor().target,
        pid: RemotePid(999999),
        cwd: "/project/wt".into(),
        cursor: RemoteCursor(cursor),
        after_sequence: cursor,
        gap: false,
        exited: false,
        chunks: vec![ReadChunk {
            cursor: RemoteCursor(cursor),
            sequence: cursor,
            data_base64: String::new(),
            bytes: format!("record-{cursor};").into_bytes(),
        }],
        agent_state: snapshot,
    }
}

fn silent_agent_read(cursor: u64, snapshot: AgentStateSnapshot) -> ReadResult {
    ReadResult {
        target: descriptor().target,
        pid: RemotePid(999999),
        cwd: "/project/wt".into(),
        cursor: RemoteCursor(cursor),
        after_sequence: cursor,
        gap: false,
        exited: false,
        chunks: Vec::new(),
        agent_state: Some(snapshot),
    }
}

/// Delivers one read response and waits (bounded) for the sink publication it must produce.
/// The notified future is created before the send, so the wait cannot miss the notification.
async fn deliver_read(
    notify: &Arc<tokio::sync::Notify>,
    tx: &mpsc::UnboundedSender<Result<ReadResult, BridgeError>>,
    read: ReadResult,
) {
    let published = notify.notified();
    tx.send(Ok(read)).expect("agent fixture retains sender");
    tokio::time::timeout(Duration::from_secs(3), published)
        .await
        .expect("agent sink publication deadline");
}

/// Waits (bounded) until the pump has issued `expected` transport reads. `Transport::read`
/// bumps the counter before that read's response can be consumed, so observing it is a
/// happens-after for everything the pump did while handling the previous response (sink
/// publication, ack updates), which keeps those assertions deterministic without polling.
async fn reads_started(fake: &Arc<Fake>, expected: usize) {
    let mut rx = fake.read_calls.subscribe();
    tokio::time::timeout(
        Duration::from_secs(3),
        rx.wait_for(|issued| *issued >= expected),
    )
    .await
    .expect("transport read deadline")
    .expect("read counter channel retained");
}

#[tokio::test]
async fn ssh_agent_state_silent_pty_read_publishes_agent_state() {
    let (runtime, hub, _dialer, tx, sink, notify) = agent_fixture(true);
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    // First read carries ordinary PTY output and no agent state.
    tx.send(Ok(agent_output(1, None))).unwrap();
    state(&mut rx, |d| d.descriptor.remote_cursor == RemoteCursor(1)).await;
    assert!(sink.published.lock().is_empty());
    assert_eq!(hub.subscribe("local-stable").unwrap().0, b"record-1;");

    // The agent then changes state while the PTY stays silent: the helper answers with no chunks
    // and an unchanged cursor, carrying only the snapshot. That state must still be published.
    let silent = AgentStateSnapshot {
        revision: RemoteCursor(2),
        state: "blocked".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: Some("Awaiting confirmation".into()),
    };
    deliver_read(&notify, &tx, silent_agent_read(1, silent)).await;

    let published = sink.published.lock().clone();
    assert_eq!(
        published.len(),
        1,
        "a silent PTY read must still publish agent state"
    );
    assert_eq!(published[0].state, "blocked");
    assert_eq!(published[0].agent.as_deref(), Some("claude"));
    assert_eq!(
        published[0].detail.as_deref(),
        Some("Awaiting confirmation")
    );
    assert_eq!(
        published[0].origin,
        crate::daemon::protocol::AgentStateOrigin::Agent
    );
    // The silent read replayed no bytes and left the PTY cursor where it was.
    assert_eq!(hub.subscribe("local-stable").unwrap().0, b"record-1;");
    assert_eq!(rx.borrow().descriptor.remote_cursor, RemoteCursor(1));
}

#[tokio::test]
async fn ssh_agent_state_dedup_suppresses_duplicate_notifications() {
    let (runtime, _, dialer, tx, sink, notify) = agent_fixture(true);
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let snap1 = AgentStateSnapshot {
        revision: RemoteCursor(1),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    deliver_read(&notify, &tx, agent_output(1, Some(snap1))).await;
    assert_eq!(sink.published.lock().len(), 1);

    // Send identical state with newer revision (periodic read / pulse). The sink must stay
    // silent here, so the wait below is the cursor, not the publication notification.
    let snap2 = AgentStateSnapshot {
        revision: RemoteCursor(2),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    tx.send(Ok(agent_output(2, Some(snap2)))).unwrap();

    // Verify session cursor advanced to 2 before the next read was issued.
    state(&mut rx, |d| d.descriptor.remote_cursor == RemoteCursor(2)).await;
    reads_started(&dialer.fake, 3).await;

    // Published count must stay 1 because state was identical (deduped)...
    assert_eq!(
        sink.published.lock().len(),
        1,
        "an identical state must not notify the sink twice"
    );
    // ...while the deduplicated revision still advances the ack.
    let acks = dialer.fake.agent_acks.lock().clone();
    assert_eq!(acks[0], RemoteCursor(0));
    assert_eq!(acks[1], RemoteCursor(1));
    assert_eq!(acks[2], RemoteCursor(2), "got: {acks:?}");
}

#[tokio::test]
async fn ssh_agent_state_provider_and_detail_change_notifies_sink() {
    let (runtime, _, dialer, tx, sink, notify) = agent_fixture(true);
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let snap1 = AgentStateSnapshot {
        revision: RemoteCursor(1),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    deliver_read(&notify, &tx, agent_output(1, Some(snap1))).await;
    assert_eq!(sink.published.lock().len(), 1);

    // AgentProviderSessionKey is snake_case on the wire, so a resumable provider session must
    // carry "session_id": the runtime publishes only sessions it can build a resume plan for.
    let claude_session = serde_json::json!({
        "id": "11111111-1111-1111-1111-111111111111",
        "key": "session_id",
    });
    let snap2 = AgentStateSnapshot {
        revision: RemoteCursor(2),
        state: "blocked".into(),
        agent: Some("claude".into()),
        provider_session: Some(claude_session),
        detail: Some("Awaiting user confirmation".into()),
    };
    deliver_read(&notify, &tx, agent_output(2, Some(snap2))).await;

    let published = sink.published.lock().clone();
    assert_eq!(published.len(), 2);
    assert_eq!(published[1].state, "blocked");
    assert_eq!(
        published[1].detail.as_deref(),
        Some("Awaiting user confirmation")
    );
    let provider = published[1]
        .provider_session
        .as_ref()
        .expect("a validated provider session must be published");
    assert_eq!(provider.id, "11111111-1111-1111-1111-111111111111");
    assert_eq!(
        provider.key,
        crate::daemon::protocol::AgentProviderSessionKey::SessionId
    );
    assert_eq!(
        published[1].origin,
        crate::daemon::protocol::AgentStateOrigin::Agent
    );
    reads_started(&dialer.fake, 3).await;
    assert_eq!(dialer.fake.agent_acks.lock()[2], RemoteCursor(2));
}

#[tokio::test]
async fn ssh_agent_state_reconnect_resumes_ack_on_post_reconnect_read() {
    let (runtime, _, dialer, tx, _sink, notify) = agent_fixture(true);
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let snap1 = AgentStateSnapshot {
        revision: RemoteCursor(42),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    deliver_read(&notify, &tx, agent_output(1, Some(snap1))).await;

    // Trigger a transport outage; the redial parks on the delay clock.
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;

    // Allow the reconnect via the dialer semaphore.
    dialer.clock.add_permits(1);
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Connected && d.generation > connected.generation
    })
    .await;

    // Read #3 is the first read issued on the new connection: read #2 died with the old one and
    // already carried 42, so the assertion is pinned to the post-reconnect call, not "any 42".
    reads_started(&dialer.fake, 3).await;
    let acks = dialer.fake.agent_acks.lock().clone();
    assert_eq!(acks[0], RemoteCursor(0));
    assert_eq!(acks[1], RemoteCursor(42));
    assert_eq!(
        acks[2],
        RemoteCursor(42),
        "post-reconnect read must resume at ack 42; got: {acks:?}"
    );
}

#[tokio::test]
async fn ssh_agent_state_fresh_runtime_replay_initializes_latest_state() {
    let (runtime1, _, _dialer1, tx1, sink1, notify1) = agent_fixture(true);
    let desc = descriptor();
    runtime1.restore(desc.clone()).unwrap();
    let mut rx1 = runtime1.subscribe("local-stable").unwrap();
    state(&mut rx1, |d| d.state == RemoteConnectionState::Connected).await;

    let retained = AgentStateSnapshot {
        revision: RemoteCursor(42),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    deliver_read(&notify1, &tx1, agent_output(1, Some(retained.clone()))).await;
    assert_eq!(sink1.published.lock().len(), 1);

    // A fresh runtime has no ack history: its first read asks to replay everything (`0`), and
    // that replayed latest state must initialize the new sink instead of leaving it empty.
    let (runtime2, _, dialer2, tx2, sink2, notify2) = agent_fixture(true);
    runtime2.restore(desc).unwrap();
    let mut rx2 = runtime2.subscribe("local-stable").unwrap();
    state(&mut rx2, |d| d.state == RemoteConnectionState::Connected).await;
    assert!(sink2.published.lock().is_empty());

    deliver_read(&notify2, &tx2, agent_output(1, Some(retained))).await;

    let published = sink2.published.lock().clone();
    assert_eq!(
        published.len(),
        1,
        "the replayed latest state must initialize the fresh sink"
    );
    assert_eq!(published[0].state, "working");
    assert_eq!(published[0].agent.as_deref(), Some("claude"));
    reads_started(&dialer2.fake, 2).await;
    let acks = dialer2.fake.agent_acks.lock().clone();
    assert_eq!(
        acks[0],
        RemoteCursor(0),
        "a fresh runtime must replay from 0"
    );
    assert_eq!(
        acks[1],
        RemoteCursor(42),
        "the replay must advance the fresh ack to the retained revision"
    );
}

#[tokio::test]
async fn ssh_agent_state_stale_revision_from_previous_generation_is_not_republished() {
    let (runtime, _, dialer, tx, sink, notify) = agent_fixture(true);
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let current = AgentStateSnapshot {
        revision: RemoteCursor(5),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    deliver_read(&notify, &tx, agent_output(1, Some(current))).await;
    assert_eq!(sink.published.lock().len(), 1);

    // The transport dies. The redial parks on the delay clock, so no read is pending and the
    // stale frame can only be consumed by the first read of the new generation.
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut rx, |d| {
        d.state == RemoteConnectionState::Reconnecting && d.generation > connected.generation
    })
    .await;
    let stale = AgentStateSnapshot {
        revision: RemoteCursor(3),
        state: "idle".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    tx.send(Ok(silent_agent_read(1, stale))).unwrap();
    dialer.clock.add_permits(1);

    // Read #3 consumes the stale frame and read #4 follows only if the session stayed healthy,
    // so the wait also proves the stale revision was discarded rather than failed on.
    reads_started(&dialer.fake, 4).await;
    assert_eq!(
        sink.published.lock().len(),
        1,
        "an already-acknowledged revision must not be republished"
    );
    let acks = dialer.fake.agent_acks.lock().clone();
    assert_eq!(acks[2], RemoteCursor(5));
    assert_eq!(
        acks[3],
        RemoteCursor(5),
        "a stale revision must not regress the acknowledged revision; got: {acks:?}"
    );
    let details = rx.borrow().clone();
    assert_eq!(details.state, RemoteConnectionState::Connected);
    assert!(details.failure.is_none());
}

#[tokio::test]
async fn ssh_agent_state_imported_live_pump_publishes_and_acks() {
    let (tx, rx) = mpsc::unbounded_channel();
    let fake = Arc::new(Fake {
        reads: Mutex::new(rx),
        describes: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        write_failure: parking_lot::Mutex::new(None),
        write_entered: parking_lot::Mutex::new(None),
        write_pause: parking_lot::Mutex::new(None),
        stop_pause: parking_lot::Mutex::new(None),
        describe_exited: std::sync::atomic::AtomicBool::new(false),
        write_history: parking_lot::Mutex::new(Vec::new()),
        resizes: AtomicUsize::new(0),
        last_resize: parking_lot::Mutex::new(None),
        supports_agent_state: true,
        agent_acks: parking_lot::Mutex::new(Vec::new()),
        read_calls: watch::channel(0usize).0,
    });
    let hub = Arc::new(TerminalOutputHub::default());
    let (sink, notify) = TestSink::new();
    let runtime = RemoteRuntime::with_sink(hub.clone(), sink.clone());

    // Live import takes the exported state verbatim (exact generation and cursor) and attaches
    // the transferred transport directly: no re-dial, no re-spawn.
    let export = RemoteExportState {
        descriptor: descriptor(),
        generation: 1,
        pending_size: None,
        pid: Some(RemotePid(8888)),
        bridge_transfer: None,
    };
    runtime
        .live_import(export, Some(fake.clone() as Arc<dyn Transport>))
        .unwrap();
    let mut sub = runtime.subscribe("local-stable").unwrap();
    state(&mut sub, |d| d.state == RemoteConnectionState::Connected).await;

    let snap = AgentStateSnapshot {
        revision: RemoteCursor(5),
        state: "idle".into(),
        agent: Some("omo".into()),
        provider_session: None,
        detail: None,
    };
    deliver_read(&notify, &tx, agent_output(1, Some(snap))).await;

    let published = sink.published.lock().clone();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].state, "idle");
    assert_eq!(published[0].agent.as_deref(), Some("omo"));
    assert_eq!(
        published[0].origin,
        crate::daemon::protocol::AgentStateOrigin::Agent
    );
    assert_eq!(hub.subscribe("local-stable").unwrap().0, b"record-1;");
    reads_started(&fake, 2).await;
    assert_eq!(
        fake.agent_acks.lock()[1],
        RemoteCursor(5),
        "the live pump must acknowledge the published revision"
    );
}

#[tokio::test]
async fn ssh_agent_state_unnegotiated_capability_rejected() {
    // supports_agent_state = false simulates old helper without agentStateV1
    let (runtime, _, _dialer, tx, sink, _notify) = agent_fixture(false);
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    let snap = AgentStateSnapshot {
        revision: RemoteCursor(1),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    // Unexpected agent_state in read response
    tx.send(Ok(agent_output(1, Some(snap)))).unwrap();
    state(&mut rx, |d| d.descriptor.remote_cursor == RemoteCursor(1)).await;

    // Must be rejected/ignored; sink receives nothing
    assert!(sink.published.lock().is_empty());
}

#[tokio::test]
async fn ssh_agent_state_invalid_state_and_zero_revision_rejected() {
    let (runtime, _, _dialer, tx, sink, _notify) = agent_fixture(true);
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;

    // 1. Invalid state string "not_a_valid_state"
    let invalid_state = AgentStateSnapshot {
        revision: RemoteCursor(1),
        state: "not_a_valid_state".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    tx.send(Ok(agent_output(1, Some(invalid_state)))).unwrap();
    state(&mut rx, |d| d.descriptor.remote_cursor == RemoteCursor(1)).await;
    assert!(sink.published.lock().is_empty());

    // 2. Zero revision
    let zero_rev = AgentStateSnapshot {
        revision: RemoteCursor(0),
        state: "working".into(),
        agent: Some("claude".into()),
        provider_session: None,
        detail: None,
    };
    tx.send(Ok(agent_output(2, Some(zero_rev)))).unwrap();
    state(&mut rx, |d| d.descriptor.remote_cursor == RemoteCursor(2)).await;
    assert!(sink.published.lock().is_empty());
}

#[tokio::test]
async fn ssh_imported_transport_recovers_after_read_disconnect() {
    // Given: a live handover owns an existing remote PTY without a new dial.
    let (runtime, hub, dialer, tx) = fixture();
    runtime.live_import(RemoteExportState {
        descriptor: descriptor(),
        generation: 7,
        pending_size: None,
        pid: Some(RemotePid(8888)),
        bridge_transfer: None,
    }, Some(dialer.fake.clone())).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    reads_started(&dialer.fake, 1).await;

    // When: the transferred transport disconnects and the retry delay is released.
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut updates, |d| d.attempts == 1 && d.generation > 7).await;
    dialer.clock.add_permits(1);
    let recovered = state(&mut updates, |d| d.state == RemoteConnectionState::Connected && d.generation > 7).await;
    tx.send(Ok(output(1, false))).unwrap();
    state(&mut updates, |d| d.descriptor.remote_cursor == RemoteCursor(1)).await;

    // Then: the same remote target produces output without a stop or replacement.
    assert_eq!(recovered.descriptor.target, descriptor().target);
    assert_eq!(hub.subscribe("local-stable").unwrap().0, b"record-1;");
    assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 0);
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ssh_close_of_gone_target_succeeds_and_forgets_the_session() {
    let gone = [
        BridgeError::RemoteTargetExpired,
        BridgeError::TargetExpired {
            expected_owner: "old-owner".into(),
            expected_epoch: crate::scoped_contracts::Epoch(1),
            actual_owner: "new-owner".into(),
            actual_epoch: crate::scoped_contracts::Epoch(2),
        },
        BridgeError::TargetNotFound,
    ];
    for error in gone {
        let (runtime, hub, dialer, tx) = fixture();
        runtime.restore(descriptor()).unwrap();
        let mut updates = runtime.subscribe("local-stable").unwrap();
        state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
        tx.send(Err(BridgeError::RemoteTargetExpired)).unwrap();
        state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;
        *dialer.failure.lock() = Some(error);
        // Nothing is left to stop, so closing a gone target is a successful close.
        runtime.close("local-stable").await.unwrap();
        assert!(runtime.details("local-stable").is_none());
        assert!(!hub.has_session("local-stable"));
        assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn ssh_close_with_unreachable_host_forgets_the_session() {
    let (runtime, hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Reconnecting).await;
    // The host stays unreachable for the close attempt too.
    *dialer.failure.lock() = Some(BridgeError::ConnectionClosed);
    let failure = runtime.close("local-stable").await.unwrap_err();
    assert_eq!(failure.kind, RemoteFailureKind::Transport);
    assert!(!runtime.contains("local-stable"));
    assert!(!hub.has_session("local-stable"));
    let dials = dialer.calls.load(Ordering::SeqCst);
    dialer.clock.add_permits(4);
    tokio::task::yield_now().await;
    assert_eq!(dialer.calls.load(Ordering::SeqCst), dials, "a closed session must not be redialed");
}

#[tokio::test]
async fn ssh_reconnect_to_exited_target_publishes_its_final_output() {
    let (runtime, hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    let (_history, mut hub_output) = hub.subscribe("local-stable").expect("hub session");
    tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Reconnecting).await;
    // While disconnected the remote program printed its last words and exited.
    dialer.fake.describe_exited.store(true, Ordering::SeqCst);
    tx.send(Ok(output(7, false))).unwrap();
    dialer.clock.add_permits(1);
    let gone = state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;
    assert_eq!(gone.failure.unwrap().kind, RemoteFailureKind::Expired);
    let published = tokio::time::timeout(Duration::from_secs(3), hub_output.recv())
        .await
        .expect("final output published before expiry")
        .unwrap();
    assert_eq!(published, b"record-7;".to_vec());
}

#[tokio::test]
async fn ssh_retry_on_expired_session_is_refused_without_redial() {
    let (runtime, _hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Err(BridgeError::RemoteTargetExpired)).unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;
    let dials = dialer.calls.load(Ordering::SeqCst);
    let failure = runtime.retry("local-stable").unwrap_err();
    assert_eq!(failure.kind, RemoteFailureKind::Expired);
    let details = runtime.details("local-stable").unwrap();
    assert_eq!(details.state, RemoteConnectionState::Expired);
    tokio::task::yield_now().await;
    assert_eq!(dialer.calls.load(Ordering::SeqCst), dials);
}

#[tokio::test]
async fn ssh_retry_while_close_stops_target_does_not_orphan_a_reconnect_loop() {
    let (runtime, hub, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    let dials_before = dialer.calls.load(Ordering::SeqCst);
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.stop_pause.lock() = Some((entered.clone(), release.clone()));
    let closing = {
        let runtime = &runtime;
        async move { runtime.close("local-stable").await }
    };
    tokio::pin!(closing);
    // Drive close until it is parked inside the stop RPC, after it published Disconnected.
    tokio::select! {
        _ = &mut closing => panic!("close finished before stop was released"),
        _ = entered.notified() => {}
    }
    assert_eq!(
        runtime.details("local-stable").unwrap().state,
        RemoteConnectionState::Disconnected
    );
    // The recovery watcher re-arms on `disconnected`; that retry must not launch a loop.
    assert!(runtime.retry("local-stable").is_err());
    release.notify_one();
    closing.await.unwrap();
    assert!(!runtime.contains("local-stable"));
    assert!(!hub.has_session("local-stable"));
    // Any orphaned loop would dial again immediately (attempts reset to 0, no delay permit needed).
    tokio::task::yield_now().await;
    assert_eq!(dialer.calls.load(Ordering::SeqCst), dials_before);
    assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ssh_imported_transport_recovers_after_control_disconnect() {
    let (runtime, hub, dialer, tx) = fixture();
    runtime.live_import(RemoteExportState {
        descriptor: descriptor(),
        generation: 7,
        pending_size: None,
        pid: Some(RemotePid(8888)),
        bridge_transfer: None,
    }, Some(dialer.fake.clone())).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    reads_started(&dialer.fake, 1).await;

    *dialer.fake.write_failure.lock() = Some(BridgeError::ConnectionClosed);
    let result = runtime.write("local-stable", 7, b"once".to_vec()).unwrap().await;
    assert!(result.is_err());
    state(&mut updates, |d| d.attempts == 1 && d.generation > 7).await;
    dialer.clock.add_permits(1);
    let recovered = state(&mut updates, |d| d.state == RemoteConnectionState::Connected && d.generation > 7).await;
    tx.send(Ok(output(1, false))).unwrap();
    state(&mut updates, |d| d.descriptor.remote_cursor == RemoteCursor(1)).await;

    assert_eq!(recovered.descriptor.target, descriptor().target);
    assert_eq!(hub.subscribe("local-stable").unwrap().0, b"record-1;");
    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 1);
    assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 0);
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ssh_target_not_found_is_retryable_disconnected_owner_unknown_not_expired() {
    let (runtime, _hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;

    tx.send(Err(BridgeError::TargetNotFound)).unwrap();
    let disconnected = state(&mut updates, |d| d.state == RemoteConnectionState::Disconnected).await;
    assert_eq!(disconnected.failure.as_ref().unwrap().kind, RemoteFailureKind::Missing);

    // retry() must be accepted because session is Disconnected (not terminal Expired)
    assert!(runtime.retry("local-stable").is_ok());
    let details = runtime.details("local-stable").unwrap();
    assert_eq!(details.state, RemoteConnectionState::Reconnecting);
}

#[tokio::test]
async fn ssh_helper_exit_proof_remains_terminal_expired() {
    let (runtime, _hub, _dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;

    // Simulate helper reporting confirmed remote target exit
    tx.send(Err(BridgeError::RemoteTargetExpired)).unwrap();
    let expired = state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;
    assert_eq!(expired.failure.as_ref().unwrap().kind, RemoteFailureKind::Expired);

    // An expired target cannot be revived by retry RPC
    let err = runtime.retry("local-stable").unwrap_err();
    assert_eq!(err.kind, RemoteFailureKind::Expired);
}

#[tokio::test]
async fn ssh_authentication_read_failure_does_not_auto_redial() {
    let (runtime, _hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    reads_started(&dialer.fake, 1).await;
    let initial_dials = dialer.calls.load(Ordering::SeqCst);
    let owner = runtime.take_recovery_owner_for_test("local-stable").unwrap();
    tx.send(Err(BridgeError::ProcessExited {
        code: Some(255),
        stderr: "Permission denied (publickey)".into(),
    }))
    .unwrap();
    let failed = state(&mut updates, |d| d.state == RemoteConnectionState::Disconnected).await;
    assert_eq!(failed.failure.unwrap().kind, RemoteFailureKind::Authentication);
    tokio::time::timeout(Duration::from_secs(3), owner)
        .await
        .expect("authentication failure terminates the recovery owner")
        .expect("recovery owner completes without panic or cancellation");
    assert_eq!(dialer.calls.load(Ordering::SeqCst), initial_dials);
    assert!(runtime.retry("local-stable").is_ok());
    assert_eq!(runtime.details("local-stable").unwrap().state, RemoteConnectionState::Reconnecting);
}

#[tokio::test]
async fn ssh_protocol_read_failure_does_not_auto_redial() {
    let (runtime, _hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    reads_started(&dialer.fake, 1).await;
    let initial_dials = dialer.calls.load(Ordering::SeqCst);
    let owner = runtime.take_recovery_owner_for_test("local-stable").unwrap();

    tx.send(Err(BridgeError::Protocol("invalid response".into())))
        .unwrap();
    let failed = state(&mut updates, |d| d.state == RemoteConnectionState::Disconnected).await;
    assert_eq!(failed.failure.unwrap().kind, RemoteFailureKind::Protocol);
    tokio::time::timeout(Duration::from_secs(3), owner)
        .await
        .expect("protocol failure terminates the recovery owner")
        .expect("recovery owner completes without panic or cancellation");
    assert_eq!(dialer.calls.load(Ordering::SeqCst), initial_dials);
    assert!(runtime.retry("local-stable").is_ok());
    assert_eq!(runtime.details("local-stable").unwrap().state, RemoteConnectionState::Reconnecting);
}

#[tokio::test]
async fn ssh_late_control_failure_preserves_terminal_expiry() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *dialer.fake.write_entered.lock() = Some(entered.clone());
    *dialer.fake.write_pause.lock() = Some(release.clone());
    *dialer.fake.write_failure.lock() = Some(BridgeError::Protocol("late failure".into()));
    let write = runtime.write("local-stable", connected.generation, b"held".to_vec()).unwrap();
    let writing = tokio::spawn(write);
    tokio::time::timeout(Duration::from_secs(3), entered.notified()).await.unwrap();

    tx.send(Err(BridgeError::RemoteTargetExpired)).unwrap();
    let expired = state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;
    release.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(3), writing).await.unwrap().unwrap().is_err());

    let current = runtime.details("local-stable").unwrap();
    assert_eq!(current.state, RemoteConnectionState::Expired);
    assert_eq!(current.failure, expired.failure);
    assert_eq!(current.generation, expired.generation);
    assert!(runtime.settle_ended("local-stable"));
}
