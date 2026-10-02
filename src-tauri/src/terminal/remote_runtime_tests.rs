use super::output_hub::TerminalOutputHub;
use super::remote::*;
use crate::{
    scoped_contracts::{Epoch, TargetRef},
    ssh::bridge::*,
};
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

struct NoopCheckpointSink;
impl CheckpointSink for NoopCheckpointSink {
    fn checkpoint<'a>(
        &'a self,
        _: &'a RemoteSessionDescriptor,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}
struct Fake {
    reads: Mutex<mpsc::UnboundedReceiver<Result<ReadResult, BridgeError>>>,
    describes: AtomicUsize,
    writes: AtomicUsize,
    stops: AtomicUsize,
    write_failure: parking_lot::Mutex<Option<BridgeError>>,
    write_entered: parking_lot::Mutex<Option<Arc<tokio::sync::Notify>>>,
    write_pause: parking_lot::Mutex<Option<Arc<tokio::sync::Notify>>>,
    write_history: parking_lot::Mutex<Vec<Vec<u8>>>,
    resizes: AtomicUsize,
    last_resize: parking_lot::Mutex<Option<(u16, u16)>>,
    supports_agent_state: bool,
    agent_acks: parking_lot::Mutex<Vec<RemoteCursor>>,
    /// Bumped inside `Transport::read` for every read the pump issues, before that read's
    /// response can be consumed. Tests await it to order assertions against the pump's handling
    /// of the previous response instead of polling the transport channel.
    read_calls: watch::Sender<usize>,
    describe_failure: parking_lot::Mutex<Option<BridgeError>>,
}
impl Transport for Fake {
    fn describe<'a>(&'a self, t: &'a TargetRef) -> Rpc<'a, DescribeResult> {
        let failure = self.describe_failure.lock().take();
        Box::pin(async move {
            self.describes.fetch_add(1, Ordering::SeqCst);
            if let Some(err) = failure {
                return Err(err);
            }
            Ok(DescribeResult {
                target: t.clone(),
                pid: RemotePid(999999),
                cwd: "/project/wt".into(),
                cols: 80,
                rows: 24,
                cursor: RemoteCursor(900),
                exited: false,
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
        Box::pin(async move {
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
    recover_calls: AtomicUsize,
    recover_result: parking_lot::Mutex<Option<Result<(Arc<Fake>, SpawnResult), BridgeError>>>,
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
    fn recover<'a>(
        &'a self,
        _d: &'a RemoteSessionDescriptor,
    ) -> Rpc<'a, (Arc<dyn Transport>, SpawnResult)> {
        Box::pin(async move {
            self.recover_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(res) = self.recover_result.lock().take() {
                match res {
                    Ok((fake, spawn)) => Ok((fake as Arc<dyn Transport>, spawn)),
                    Err(e) => Err(e),
                }
            } else {
                Err(BridgeError::Protocol(
                    "Remote helper does not advertise 'ptyRecoveryV1' capability".into(),
                ))
            }
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
fn make_fake() -> (Arc<Fake>, mpsc::UnboundedSender<Result<ReadResult, BridgeError>>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let fake = Arc::new(Fake {
        reads: Mutex::new(rx),
        describes: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        write_failure: parking_lot::Mutex::new(None),
        write_entered: parking_lot::Mutex::new(None),
        write_pause: parking_lot::Mutex::new(None),
        write_history: parking_lot::Mutex::new(Vec::new()),
        resizes: AtomicUsize::new(0),
        last_resize: parking_lot::Mutex::new(None),
        supports_agent_state: false,
        agent_acks: parking_lot::Mutex::new(Vec::new()),
        read_calls: watch::channel(0usize).0,
        describe_failure: parking_lot::Mutex::new(None),
    });
    (fake, tx)
}
fn fixture() -> (
    RemoteRuntime,
    Arc<TerminalOutputHub>,
    Arc<Dialer>,
    mpsc::UnboundedSender<Result<ReadResult, BridgeError>>,
) {
    let (fake, tx) = make_fake();
    let dialer = Arc::new(Dialer {
        fake,
        calls: AtomicUsize::new(0),
        clock: Arc::new(Semaphore::new(0)),
        failure: parking_lot::Mutex::new(None),
        recover_calls: AtomicUsize::new(0),
        recover_result: parking_lot::Mutex::new(None),
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
async fn ssh_reconnect_safety_retry_cap_and_terminal_failure() {
    let (runtime, _, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    for attempt in 0..=5 {
        state(&mut rx, |d| {
            d.state == RemoteConnectionState::Connected && d.attempts == attempt
        })
        .await;
        tx.send(Err(BridgeError::ConnectionClosed)).unwrap();
        if attempt < 5 {
            state(&mut rx, |d| {
                d.state == RemoteConnectionState::Reconnecting && d.attempts == attempt + 1
            })
            .await;
            dialer.clock.add_permits(1);
        } else {
            state(&mut rx, |d| d.state == RemoteConnectionState::Disconnected).await;
        }
    }
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 6);
    runtime.retry("local-stable").unwrap();
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Err(BridgeError::RemoteTargetExpired)).unwrap();
    let expired = state(&mut rx, |d| d.state == RemoteConnectionState::Expired).await;
    assert_eq!(expired.failure.unwrap().kind, RemoteFailureKind::Expired);
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 7);
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
        write_history: parking_lot::Mutex::new(Vec::new()),
        resizes: AtomicUsize::new(0),
        last_resize: parking_lot::Mutex::new(None),
        supports_agent_state,
        agent_acks: parking_lot::Mutex::new(Vec::new()),
        read_calls: watch::channel(0usize).0,
        describe_failure: parking_lot::Mutex::new(None),
    });
    let dialer = Arc::new(Dialer {
        fake,
        calls: AtomicUsize::new(0),
        clock: Arc::new(Semaphore::new(0)),
        failure: parking_lot::Mutex::new(None),
        recover_calls: AtomicUsize::new(0),
        recover_result: parking_lot::Mutex::new(None),
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
        write_history: parking_lot::Mutex::new(Vec::new()),
        resizes: AtomicUsize::new(0),
        last_resize: parking_lot::Mutex::new(None),
        supports_agent_state: true,
        agent_acks: parking_lot::Mutex::new(Vec::new()),
        read_calls: watch::channel(0usize).0,
        describe_failure: parking_lot::Mutex::new(None),
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
async fn ssh_close_preserves_expired_target_classification() {
    let (runtime, _hub, dialer, tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    tx.send(Err(BridgeError::RemoteTargetExpired)).unwrap();
    state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;
    *dialer.failure.lock() = Some(BridgeError::RemoteTargetExpired);
    let failure = runtime.close("local-stable").await.unwrap_err();
    assert_eq!(failure.kind, RemoteFailureKind::Expired);
    let details = runtime.details("local-stable").unwrap();
    assert_eq!(details.state, RemoteConnectionState::Expired);
    assert_eq!(dialer.fake.stops.load(Ordering::SeqCst), 0);
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
async fn reboot_recovery_target_replacement_keeps_backend_id() {
    let (runtime, _hub, dialer, _tx) = fixture();
    runtime.set_checkpoint_sink(Arc::new(NoopCheckpointSink));
    let initial_desc = descriptor();
    let new_target = TargetRef {
        host_id: "host".into(),
        owner_id: "new-owner".into(),
        epoch: Epoch(2),
        backend_session_id: "remote-recovered".into(),
    };
    let (rec_fake, rec_tx) = make_fake();
    *dialer.failure.lock() = Some(BridgeError::TargetExpired {
        expected_owner: "owner".into(),
        expected_epoch: Epoch(1),
        actual_owner: "new-owner".into(),
        actual_epoch: Epoch(2),
    });
    *dialer.recover_result.lock() = Some(Ok((
        rec_fake.clone(),
        SpawnResult {
            target: new_target.clone(),
            pid: RemotePid(77777),
        },
    )));

    runtime.restore(initial_desc).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    request_reboot_recovery(&runtime, &dialer, &mut updates).await;
    let connected = state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;

    // Backend session ID must be preserved exactly:
    assert_eq!(connected.descriptor.backend_session_id, "local-stable");
    assert_eq!(connected.descriptor.client_request_id, "request-original");
    // Target was replaced with recovered target:
    assert_eq!(connected.descriptor.target, new_target);
    // Remote PID was updated:
    assert_eq!(connected.pid, Some(RemotePid(999999)));
    assert_eq!(dialer.recover_calls.load(Ordering::SeqCst), 1);
    drop(rec_tx);
}

#[tokio::test]
async fn reboot_recovery_saved_recipe_descriptor_target_update() {
    let (runtime, _hub, dialer, _tx) = fixture();
    let new_target = TargetRef {
        host_id: "host".into(),
        owner_id: "new-owner".into(),
        epoch: Epoch(2),
        backend_session_id: "remote-recovered".into(),
    };
    let (rec_fake, rec_tx) = make_fake();
    *dialer.failure.lock() = Some(BridgeError::RemoteTargetExpired);
    *dialer.recover_result.lock() = Some(Ok((
        rec_fake.clone(),
        SpawnResult {
            target: new_target.clone(),
            pid: RemotePid(11111),
        },
    )));

    struct TestSink {
        saved: parking_lot::Mutex<Vec<RemoteSessionDescriptor>>,
        checkpoint_saw_connected: std::sync::atomic::AtomicBool,
        sub: parking_lot::Mutex<Option<watch::Receiver<RemoteSessionDetails>>>,
    }
    impl CheckpointSink for TestSink {
        fn checkpoint<'a>(
            &'a self,
            desc: &'a RemoteSessionDescriptor,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            let desc_clone = desc.clone();
            Box::pin(async move {
                if let Some(ref sub) = *self.sub.lock() {
                    let cur = sub.borrow();
                    if cur.state == RemoteConnectionState::Connected {
                        self.checkpoint_saw_connected.store(true, Ordering::SeqCst);
                    }
                }
                self.saved.lock().push(desc_clone);
                Ok(())
            })
        }
    }

    let sink = Arc::new(TestSink {
        saved: parking_lot::Mutex::new(Vec::new()),
        checkpoint_saw_connected: std::sync::atomic::AtomicBool::new(false),
        sub: parking_lot::Mutex::new(None),
    });
    runtime.set_checkpoint_sink(sink.clone());

    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    *sink.sub.lock() = Some(updates.clone());

    request_reboot_recovery(&runtime, &dialer, &mut updates).await;
    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;

    let saved = sink.saved.lock().clone();
    assert_eq!(saved.len(), 1, "exactly one checkpoint on recovery");
    assert_eq!(saved[0].target, new_target, "checkpointed updated descriptor target");
    assert!(!sink.checkpoint_saw_connected.load(Ordering::SeqCst), "checkpoint ran BEFORE status became Connected");
    drop(rec_tx);
}

#[tokio::test]
async fn reboot_recovery_checkpoint_failure_aborts_without_claiming_connected() {
    let (runtime, _hub, dialer, _tx) = fixture();
    let new_target = TargetRef {
        host_id: "host".into(),
        owner_id: "new-owner".into(),
        epoch: Epoch(2),
        backend_session_id: "remote-recovered".into(),
    };
    let (rec_fake, rec_tx) = make_fake();
    *dialer.failure.lock() = Some(BridgeError::RemoteTargetExpired);
    *dialer.recover_result.lock() = Some(Ok((
        rec_fake.clone(),
        SpawnResult {
            target: new_target.clone(),
            pid: RemotePid(11111),
        },
    )));

    struct FailingSink;
    impl CheckpointSink for FailingSink {
        fn checkpoint<'a>(
            &'a self,
            _: &'a RemoteSessionDescriptor,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                Err("Disk quota exceeded".into())
            })
        }
    }
    runtime.set_checkpoint_sink(Arc::new(FailingSink));

    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    request_reboot_recovery(&runtime, &dialer, &mut updates).await;

    // Must never claim Connected! Must transition to terminal failure
    let details = state(&mut updates, |d| d.failure.is_some()).await;
    assert_ne!(details.state, RemoteConnectionState::Connected);
    assert_eq!(details.failure.as_ref().unwrap().kind, RemoteFailureKind::Protocol);
    assert!(details.failure.as_ref().unwrap().message.contains("Disk quota exceeded"));
    // A failed checkpoint must leave the previously persisted identity untouched.
    assert_eq!(details.descriptor.target, descriptor().target);
    assert_eq!(rec_fake.stops.load(Ordering::SeqCst), 0);
    drop(rec_tx);
}

#[tokio::test]
async fn reboot_recovery_concurrent_retry_coalescing() {
    let (_r, _hub, dialer, _tx) = fixture();
    let new_target = TargetRef {
        host_id: "host".into(),
        owner_id: "new-owner".into(),
        epoch: Epoch(2),
        backend_session_id: "remote-recovered".into(),
    };
    let (rec_fake, rec_tx) = make_fake();
    *dialer.failure.lock() = Some(BridgeError::RemoteTargetExpired);
    let recover_gate = Arc::new(tokio::sync::Notify::new());
    let recover_gate_clone = recover_gate.clone();

    struct GatedConnector {
        dialer: Arc<Dialer>,
        rec_fake: Arc<Fake>,
        new_target: TargetRef,
        gate: Arc<tokio::sync::Notify>,
        recover_entered: Arc<tokio::sync::Notify>,
    }
    impl Connector for GatedConnector {
        fn connect<'a>(&'a self, d: &'a RemoteSessionDescriptor) -> Rpc<'a, Arc<dyn Transport>> {
            self.dialer.connect(d)
        }
        fn recover<'a>(
            &'a self,
            _d: &'a RemoteSessionDescriptor,
        ) -> Rpc<'a, (Arc<dyn Transport>, SpawnResult)> {
            let gate = self.gate.clone();
            let entered = self.recover_entered.clone();
            let fake = self.rec_fake.clone();
            let target = self.new_target.clone();
            self.dialer.recover_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                entered.notify_one();
                gate.notified().await;
                Ok((fake as Arc<dyn Transport>, SpawnResult { target, pid: RemotePid(55555) }))
            })
        }
        fn delay(&self, a: u32) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            self.dialer.delay(a)
        }
    }

    let recover_entered = Arc::new(tokio::sync::Notify::new());
    let gated = Arc::new(GatedConnector {
        dialer: dialer.clone(),
        rec_fake,
        new_target,
        gate: recover_gate,
        recover_entered: recover_entered.clone(),
    });
    let hub = Arc::new(TerminalOutputHub::default());
    let runtime = RemoteRuntime::with_connector(hub, gated);
    runtime.set_checkpoint_sink(Arc::new(NoopCheckpointSink));

    runtime.restore(descriptor()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();

    let entered = recover_entered.notified();
    tokio::pin!(entered);
    entered.as_mut().enable();
    request_reboot_recovery(&runtime, &dialer, &mut updates).await;
    tokio::time::timeout(Duration::from_secs(5), entered).await.unwrap();

    // While recovery is in-flight (Reconnecting), concurrent calls to retry must coalesce:
    for _ in 0..5 {
        assert!(runtime.retry("local-stable").is_ok());
    }

    // Now release recovery
    recover_gate_clone.notify_one();

    state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    // Exactly 1 recovery was performed:
    assert_eq!(dialer.recover_calls.load(Ordering::SeqCst), 1);
    drop(rec_tx);
}

#[tokio::test]
async fn reboot_recovery_cursor_generation_reset_and_old_input_rejected() {
    let (runtime, _hub, dialer, _tx) = fixture();
    runtime.set_checkpoint_sink(Arc::new(NoopCheckpointSink));
    let mut initial_desc = descriptor();
    initial_desc.remote_cursor = RemoteCursor(500);

    let new_target = TargetRef {
        host_id: "host".into(),
        owner_id: "new-owner".into(),
        epoch: Epoch(2),
        backend_session_id: "remote-recovered".into(),
    };
    let (rec_fake, rec_tx) = make_fake();
    *dialer.failure.lock() = Some(BridgeError::RemoteTargetExpired);
    *dialer.recover_result.lock() = Some(Ok((
        rec_fake.clone(),
        SpawnResult {
            target: new_target.clone(),
            pid: RemotePid(99999),
        },
    )));

    runtime.restore(initial_desc).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    request_reboot_recovery(&runtime, &dialer, &mut updates).await;
    let connected = state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;

    // Remote cursor must be reset to 0:
    assert_eq!(connected.descriptor.remote_cursor, RemoteCursor(0));
    assert!(connected.generation > 1);
    // Gap must be emitted:
    assert!(connected.replay_gap.is_some());
    let gap = connected.replay_gap.unwrap();
    assert_eq!(gap.requested_after_cursor, RemoteCursor(500));
    assert_eq!(gap.available_from_cursor, RemoteCursor(0));

    // Stale generation write (generation 1) must be rejected with StaleGeneration:
    let old_write = runtime.write("local-stable", 1, b"old-input".to_vec());
    let err = match old_write {
        Err(e) => e,
        Ok(_) => panic!("expected stale generation write to fail"),
    };
    assert_eq!(err.kind, RemoteFailureKind::StaleGeneration);

    let current_write = runtime.write("local-stable", connected.generation, b"new-input".to_vec());
    assert!(current_write.is_ok());
    let res = current_write.unwrap().await;
    assert!(res.is_ok());
    assert_eq!(rec_fake.writes.load(Ordering::SeqCst), 1);
    drop(rec_tx);
}

#[tokio::test]
async fn reboot_recovery_checkpoint_fails_once_then_retry_succeeds() {
    let (_unused_runtime, _hub, dialer, _tx) = fixture();
    let initial_desc = descriptor();
    let new_target = TargetRef {
        host_id: "host".into(),
        owner_id: "new-owner".into(),
        epoch: Epoch(2),
        backend_session_id: "remote-recovered".into(),
    };
    let (rec_fake, rec_tx) = make_fake();
    *dialer.failure.lock() = Some(BridgeError::RemoteTargetExpired);

    let spawn_res = SpawnResult {
        target: new_target.clone(),
        pid: RemotePid(22222),
    };
    let rec_fake_clone = rec_fake.clone();
    let spawn_res_clone = spawn_res.clone();

    struct FlakySink {
        attempts: AtomicUsize,
        saved: parking_lot::Mutex<Vec<RemoteSessionDescriptor>>,
    }
    impl CheckpointSink for FlakySink {
        fn checkpoint<'a>(
            &'a self,
            desc: &'a RemoteSessionDescriptor,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            let desc_clone = desc.clone();
            Box::pin(async move {
                if attempt == 0 {
                    Err("Temporary disk lock".into())
                } else {
                    self.saved.lock().push(desc_clone);
                    Ok(())
                }
            })
        }
    }

    let sink = Arc::new(FlakySink {
        attempts: AtomicUsize::new(0),
        saved: parking_lot::Mutex::new(Vec::new()),
    });

    struct IdempotentConnector {
        dialer: Arc<Dialer>,
        rec_fake: Arc<Fake>,
        spawn: SpawnResult,
    }
    impl Connector for IdempotentConnector {
        fn connect<'a>(&'a self, d: &'a RemoteSessionDescriptor) -> Rpc<'a, Arc<dyn Transport>> {
            Box::pin(async move {
                self.dialer.calls.fetch_add(1, Ordering::SeqCst);
                if d.target == self.spawn.target {
                    Ok(self.rec_fake.clone() as Arc<dyn Transport>)
                } else {
                    Err(BridgeError::RemoteTargetExpired)
                }
            })
        }
        fn recover<'a>(
            &'a self,
            _d: &'a RemoteSessionDescriptor,
        ) -> Rpc<'a, (Arc<dyn Transport>, SpawnResult)> {
            self.dialer.recover_calls.fetch_add(1, Ordering::SeqCst);
            let fake = self.rec_fake.clone();
            let spawn = self.spawn.clone();
            Box::pin(async move {
                Ok((fake as Arc<dyn Transport>, spawn))
            })
        }
        fn delay(&self, a: u32) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            self.dialer.delay(a)
        }
    }

    let hub = Arc::new(TerminalOutputHub::default());
    let runtime = RemoteRuntime::with_connector(
        hub,
        Arc::new(IdempotentConnector {
            dialer: dialer.clone(),
            rec_fake: rec_fake_clone,
            spawn: spawn_res_clone,
        }),
    );
    runtime.set_checkpoint_sink(sink.clone());

    runtime.restore(initial_desc.clone()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    request_reboot_recovery(&runtime, &dialer, &mut updates).await;

    // 1. First recovery attempt fails during checkpoint:
    let failed = state(&mut updates, |d| d.failure.is_some()).await;
    assert_ne!(failed.state, RemoteConnectionState::Connected);
    assert_eq!(failed.failure.as_ref().unwrap().kind, RemoteFailureKind::Protocol);
    // Descriptor target must remain unchanged on checkpoint failure:
    assert_eq!(failed.descriptor.target, initial_desc.target);
    assert_eq!(sink.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(sink.saved.lock().len(), 0);

    // 2. Retry triggers second recovery attempt:
    assert!(runtime.retry("local-stable").is_ok());

    // Second recovery succeeds after second checkpoint:
    let connected = state(&mut updates, |d| d.state == RemoteConnectionState::Connected).await;
    assert_eq!(connected.state, RemoteConnectionState::Connected);
    assert_eq!(connected.descriptor.target, new_target);
    assert_eq!(sink.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(sink.saved.lock().len(), 1);
    assert_eq!(sink.saved.lock()[0].target, new_target);

    // Stale generation 1 write must be rejected:
    let old_write = runtime.write("local-stable", 1, b"old".to_vec());
    let err = match old_write {
        Err(e) => e,
        Ok(_) => panic!("expected stale generation write to fail"),
    };
    assert_eq!(err.kind, RemoteFailureKind::StaleGeneration);
    drop(rec_tx);
}

#[tokio::test]
async fn reboot_recovery_legacy_no_capability_safe() {
    let (runtime, _hub, dialer, _tx) = fixture();
    let initial_desc = descriptor();
    *dialer.failure.lock() = Some(BridgeError::TargetExpired {
        expected_owner: "owner".into(),
        expected_epoch: Epoch(1),
        actual_owner: "new-owner".into(),
        actual_epoch: Epoch(2),
    });
    *dialer.recover_result.lock() = Some(Err(BridgeError::RecoveryUnsupported));

    runtime.restore(initial_desc.clone()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    request_reboot_recovery(&runtime, &dialer, &mut updates).await;
    let terminal = state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;

    // Preserves original Expired failure so legacy behavior is untouched:
    assert_eq!(terminal.state, RemoteConnectionState::Expired);
    assert_eq!(terminal.failure.as_ref().unwrap().kind, RemoteFailureKind::Expired);
    // Target was NOT changed:
    assert_eq!(terminal.descriptor.target, initial_desc.target);
    // No arbitrary spawn:
    assert_eq!(terminal.pid, None);
}

async fn request_reboot_recovery(
    runtime: &RemoteRuntime,
    dialer: &Dialer,
    updates: &mut watch::Receiver<RemoteSessionDetails>,
) {
    state(updates, |d| d.state == RemoteConnectionState::Expired).await;
    assert_eq!(dialer.recover_calls.load(Ordering::SeqCst), 0, "restoration must not launch an agent");
    *dialer.failure.lock() = Some(BridgeError::RemoteTargetExpired);
    runtime.retry("local-stable").unwrap();
}
#[tokio::test]
async fn reboot_recovery_no_recovery_after_target_not_found_natural_exit() {
    let (runtime, _hub, dialer, _tx) = fixture();
    let initial_desc = descriptor();
    // Process exited naturally: describe returns TargetNotFound
    *dialer.fake.describe_failure.lock() = Some(BridgeError::TargetNotFound);

    runtime.restore(initial_desc.clone()).unwrap();
    let mut updates = runtime.subscribe("local-stable").unwrap();
    let terminal = state(&mut updates, |d| d.state == RemoteConnectionState::Expired).await;

    // TargetNotFound must NEVER trigger recover:
    assert_eq!(dialer.recover_calls.load(Ordering::SeqCst), 0);
    assert_eq!(terminal.state, RemoteConnectionState::Expired);
    assert_eq!(terminal.failure.as_ref().unwrap().kind, RemoteFailureKind::Missing);
    assert_eq!(terminal.descriptor.target, initial_desc.target);
}
