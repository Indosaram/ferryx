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
                exited: false,
            })
        })
    }
    fn read<'a>(&'a self, _: &'a TargetRef, _: RemoteCursor) -> Rpc<'a, ReadResult> {
        Box::pin(async move {
            self.reads
                .lock()
                .await
                .recv()
                .await
                .expect("test retains sender")
        })
    }
    fn write<'a>(&'a self, _: &'a TargetRef, _: &'a [u8]) -> Rpc<'a, ()> {
        Box::pin(async move {
            self.writes.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.write_failure.lock().take() {
                return Err(error);
            }
            Ok(())
        })
    }
    fn resize<'a>(&'a self, _: &'a TargetRef, _: u16, _: u16) -> Rpc<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn stop<'a>(&'a self, _: &'a TargetRef) -> Rpc<'a, ()> {
        Box::pin(async move {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
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
    }
}

#[tokio::test]
async fn ssh_reconnect_safety_control_failure_interrupts_pending_read() {
    let (runtime, _, dialer, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut rx = runtime.subscribe("local-stable").unwrap();
    let connected = state(&mut rx, |d| d.state == RemoteConnectionState::Connected).await;
    *dialer.fake.write_failure.lock() = Some(BridgeError::ConnectionClosed);
    let error = runtime.write("local-stable", connected.generation, b"once".to_vec())
        .unwrap().await.unwrap_err();
    assert_eq!(error.kind, RemoteFailureKind::Transport);
    state(&mut rx, |d| d.attempts == 1 && d.generation > connected.generation).await;
    dialer.clock.add_permits(1);
    state(&mut rx, |d| d.state == RemoteConnectionState::Connected && d.generation > connected.generation).await;
    assert_eq!(dialer.fake.writes.load(Ordering::SeqCst), 1);
    assert_eq!(dialer.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ssh_daemon_restart_rejects_duplicate_target_controller() {
    let (runtime, _, _, _tx) = fixture();
    runtime.restore(descriptor()).unwrap();
    let mut duplicate = descriptor();
    duplicate.backend_session_id = "second-local-id".into();
    assert_eq!(runtime.restore(duplicate).unwrap_err().kind, RemoteFailureKind::Protocol);
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
    for message in ["Permission denied", "authentication", "connection refused", "connection timed out", "no route to host", "connection reset"] {
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
        (IpcErrorCode::IoError, serde_json::json!({"stage": "transport"}), RemoteFailureKind::Transport),
        (IpcErrorCode::IoError, serde_json::json!({"stage": "execution", "exitCode": 255, "stderr": "Permission denied (publickey)"}), RemoteFailureKind::Authentication),
        (IpcErrorCode::IoError, serde_json::json!({"stage": "execution", "exitCode": 255, "stderr": "Connection refused"}), RemoteFailureKind::Transport),
        (IpcErrorCode::InvalidArgument, serde_json::json!({"stage": "transport"}), RemoteFailureKind::Protocol),
        (IpcErrorCode::IoError, serde_json::json!({"stage": "startup"}), RemoteFailureKind::Protocol),
        (IpcErrorCode::CliExecutableNotFound, serde_json::json!({"stage": "helper_missing"}), RemoteFailureKind::Missing),
    ];
    for (code, details, expected) in cases {
        let error = IpcError::new(code, "authentication connection refused").with_details(details);
        assert_eq!(RemoteFailure::from_bridge(&BridgeError::from(error)).kind, expected);
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
    assert_eq!(
        hub.subscribe("local-stable").unwrap().0,
        b"record-8;"
    );
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
