use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferryx_host::transport::{serve, uuid_hex};
use ferryx_host::{HostInfo, HostShared};
use fxsh::frame::{Frame, FLAG_ERROR, FLAG_RESPONSE};
use fxsh::messages::*;
use fxsh::types::*;
use fxsh::{decode_frame, state_digest, Decoded, ErrorDetail, Message, Uuid, CAP_V1_REQUIRED};
use session_core::client::{ClientOut, ReplicaClient};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

const WAIT: Duration = Duration::from_secs(20);

type Writer = Box<dyn AsyncWrite + Unpin + Send>;

struct Client {
    w: Writer,
    rx: UnboundedReceiver<Frame>,
    backlog: VecDeque<Frame>,
    next_rid: u64,
    me: Uuid,
}

async fn connect(endpoint: &str) -> (Writer, Box<dyn AsyncRead + Unpin + Send>) {
    #[cfg(unix)]
    {
        let s = tokio::net::UnixStream::connect(endpoint).await.expect("connect");
        let (r, w) = s.into_split();
        (Box::new(w), Box::new(r))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        let started = Instant::now();
        let pipe = loop {
            match ClientOptions::new().open(endpoint) {
                Ok(p) => break p,
                Err(e) if started.elapsed() < WAIT => {
                    let _ = e;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => panic!("connect: {e}"),
            }
        };
        let (r, w) = tokio::io::split(pipe);
        (Box::new(w), Box::new(r))
    }
}

impl Client {
    async fn open(endpoint: &str, me: u8) -> Client {
        let (w, mut r) = connect(endpoint).await;
        let (tx, rx) = unbounded_channel();
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                while let Ok(Decoded::Frame { consumed, frame, .. }) = decode_frame(&buf) {
                    buf.drain(..consumed);
                    if tx.send(frame).is_err() {
                        return;
                    }
                }
                match r.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
        });
        let mut c = Client { w, rx, backlog: VecDeque::new(), next_rid: 0, me: Uuid([me; 16]) };
        let ack = c.request(Uuid::default(), Message::Hello(Hello { major: 1, minor: 0, capabilities: CAP_V1_REQUIRED, client_kind: ClientKind::PolicyDaemon, client_instance_id: c.me })).await;
        assert!(matches!(ack.message, Message::HelloAck(_)), "{ack:?}");
        c
    }

    async fn send(&mut self, session: Uuid, message: Message) -> u64 {
        self.next_rid += 1;
        let rid = self.next_rid;
        let bytes = Frame { session_id: session, request_id: rid, flags: 0, message }.encode();
        self.w.write_all(&bytes).await.expect("write");
        rid
    }

    async fn next_frame(&mut self, deadline: Instant) -> Option<Frame> {
        if let Some(f) = self.backlog.pop_front() {
            return Some(f);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        tokio::time::timeout(left, self.rx.recv()).await.ok().flatten()
    }

    async fn request(&mut self, session: Uuid, message: Message) -> Frame {
        let rid = self.send(session, message).await;
        let deadline = Instant::now() + WAIT;
        let mut skipped = Vec::new();
        loop {
            let f = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), self.rx.recv()).await.expect("response timeout").expect("connection closed");
            if f.flags & FLAG_RESPONSE != 0 && f.request_id == rid {
                for s in skipped.into_iter().rev() {
                    self.backlog.push_front(s);
                }
                return f;
            }
            skipped.push(f);
            for s in skipped.drain(..) {
                self.backlog.push_back(s);
            }
        }
    }

    async fn drive(&mut self, rc: &mut ReplicaClient, mut until: impl FnMut(&ReplicaClient) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !until(rc) {
            let f = self.next_frame(deadline).await.unwrap_or_else(|| panic!("timed out; replica revision {}, screen {:?}", rc.local_revision, row_texts(rc).into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>()));
            let out = match f.message {
                Message::SnapshotFrame(s) => rc.on_snapshot(&s),
                Message::Delta(d) => rc.on_delta(&d),
                _ => None,
            };
            if matches!(out, Some(ClientOut::Resync) | Some(ClientOut::DigestMismatch)) {
                let sid = rc.subscription_id.expect("subscription");
                self.send(f.session_id, Message::Resync(Resync { subscription_id: sid })).await;
            }
        }
    }

    async fn event_where(&mut self, mut pred: impl FnMut(&Frame) -> bool) -> Frame {
        let deadline = Instant::now() + WAIT;
        loop {
            let f = self.next_frame(deadline).await.expect("event timeout");
            if pred(&f) {
                return f;
            }
        }
    }
}

fn row_texts(rc: &ReplicaClient) -> Vec<String> {
    rc.replica
        .as_ref()
        .map(|r| r.screen.iter().map(|row| row.cells.iter().map(|c| char::from_u32(c.codepoint).filter(|_| c.codepoint != 0).unwrap_or(' ')).collect::<String>().trim_end().to_owned()).collect())
        .unwrap_or_default()
}

fn error_code(f: &Frame) -> Option<u16> {
    match &f.message {
        Message::Error(e) if f.flags & FLAG_ERROR != 0 => Some(e.code),
        _ => None,
    }
}

async fn start_host() -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let instance = Uuid(*uuid::Uuid::new_v4().as_bytes());
    #[cfg(unix)]
    let endpoint = {
        use std::os::unix::fs::PermissionsExt;
        let private = dir.path().join("ferryx");
        std::fs::create_dir(&private).expect("create endpoint dir");
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).expect("chmod 0700");
        private.join("h.sock").to_string_lossy().into_owned()
    };
    #[cfg(windows)]
    let endpoint = format!(r"\\.\pipe\ferryx-host-test-{}", uuid_hex(&instance));
    let info = Arc::new(HostInfo { supervisor: fxsh::types::Supervisor::Systemd, pid: std::process::id(), process_start_time: 0, version: "test".into() });
    let host = HostShared::new(instance);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let ep = endpoint.clone();
    tokio::spawn(async move {
        let r = serve(host, info, &ep, move || {
            let _ = ready_tx.send(());
        })
        .await;
        panic!("host stopped: {r:?}");
    });
    tokio::time::timeout(WAIT, ready_rx).await.expect("host ready").expect("ready");
    (endpoint, dir)
}

fn shell() -> (String, Vec<String>, Vec<(String, String)>, &'static str) {
    #[cfg(unix)]
    {
        ("/bin/sh".into(), vec![], vec![("PS1".into(), "$ ".into()), ("TERM".into(), "xterm-256color".into())], "echo FERRYX_E2E_OK\n")
    }
    #[cfg(windows)]
    {
        ("cmd.exe".into(), vec!["/Q".into()], vec![], "echo FERRYX_E2E_OK\r\n")
    }
}

async fn spawn_session(c: &mut Client, program: String, args: Vec<String>, env: Vec<(String, String)>) -> (Frame, OperationId) {
    let op = match c.request(Uuid::default(), Message::ReserveOperation(ReserveOperation { kind: OperationKind::Spawn })).await.message {
        Message::OperationReserved(o) => o.operation_id,
        other => panic!("{other:?}"),
    };
    let spawn = Spawn { operation_id: op.clone(), cols: 80, rows: 24, program, args, env, cwd: String::new() };
    (c.request(Uuid::default(), Message::Spawn(spawn)).await, op)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_lifecycle_over_the_real_transport() {
    let (endpoint, _dir) = start_host().await;
    let mut c = Client::open(&endpoint, 1).await;
    let (program, args, env, command) = shell();

    let (spawned, op) = spawn_session(&mut c, program.clone(), args.clone(), env.clone()).await;
    let Message::SpawnResult(sp) = spawned.message else { panic!("{spawned:?}") };
    assert!(sp.created && sp.pid > 0);
    let sid = sp.session_id;

    let replay = c.request(Uuid::default(), Message::Spawn(Spawn { operation_id: op.clone(), cols: 80, rows: 24, program, args, env, cwd: String::new() })).await;
    let Message::SpawnResult(again) = replay.message else { panic!("{replay:?}") };
    assert!(!again.created && again.session_id == sid && again.session_incarnation == sp.session_incarnation, "an idempotent replay must name the same session");

    let subscriber = Uuid([7; 16]);
    let ack = c.request(sid, Message::Subscribe(Subscribe { subscriber_id: subscriber, attach_seq: 1, client_known_revision: None, client_known_incarnation: None })).await;
    let Message::SubscribeAck(ack) = ack.message else { panic!("{ack:?}") };
    assert!(ack.snapshot_follows);
    let mut rc = ReplicaClient::new(sp.session_incarnation);
    rc.adopt(ack.subscription_id, true, ack.revision);
    c.drive(&mut rc, |rc| rc.replica.is_some()).await;

    let granted = c.request(sid, Message::AcquireLease(AcquireLease { client_instance_id: c.me, lease_request_id: 1, scope: Scope::Input })).await;
    let Message::LeaseGranted(g) = granted.message else { panic!("{granted:?}") };
    let bytes = command.as_bytes().to_vec();
    let crc = fxsh::input_crc32c(&bytes);
    let w = c.request(sid, Message::WriteInput(WriteInput { client_instance_id: c.me, epoch: g.epoch, start: 0, bytes: fxsh::Bytes(bytes.clone()), crc32c: crc })).await;
    let Message::InputAck(a) = w.message else { panic!("{w:?}") };
    assert_eq!(a.accepted, bytes.len() as u64);

    c.drive(&mut rc, |rc| row_texts(rc).iter().any(|t| t == "FERRYX_E2E_OK")).await;

    let retry = c.request(sid, Message::WriteInput(WriteInput { client_instance_id: c.me, epoch: g.epoch, start: 0, bytes: fxsh::Bytes(bytes.clone()), crc32c: crc })).await;
    let Message::InputAck(a2) = retry.message else { panic!("{retry:?}") };
    assert_eq!(a2.accepted, bytes.len() as u64, "a retransmission must not be accepted twice");

    let stale = c.request(sid, Message::WriteInput(WriteInput { client_instance_id: c.me, epoch: g.epoch + 5, start: 0, bytes: fxsh::Bytes(vec![b'x']), crc32c: fxsh::input_crc32c(b"x") })).await;
    assert_eq!(error_code(&stale), Some(1), "STALE_LEASE");

    let mut state = None;
    for _ in 0..20 {
        let st = c.request(sid, Message::GetSessionState).await;
        let Message::SessionState(st) = st.message else { panic!("{st:?}") };
        let target = st.state_revision;
        c.drive(&mut rc, |rc| rc.local_revision >= target).await;
        if rc.local_revision == target {
            state = Some(st);
            break;
        }
    }
    let state = state.expect("host and replica reach the same revision once output is idle");
    assert_eq!(state_digest(&rc.replica.as_ref().unwrap().to_body()), state.state_digest, "the client replica equals the host state");
    assert_eq!(state.input_committed, bytes.len() as u64, "the retransmission must not have been written again");

    let mut c2 = Client::open(&endpoint, 2).await;
    let known = c2
        .request(sid, Message::Subscribe(Subscribe { subscriber_id: Uuid([8; 16]), attach_seq: 1, client_known_revision: Some(state.state_revision), client_known_incarnation: Some(sp.session_incarnation) }))
        .await;
    let Message::SubscribeAck(known) = known.message else { panic!("{known:?}") };
    assert!(!known.snapshot_follows && known.revision == state.state_revision, "a client that already holds the current state needs no snapshot");

    let listed = c2.request(Uuid::default(), Message::ListSessions(ListSessions { request_token: (Uuid([9; 16]), 1) })).await;
    let Message::SessionList(list) = listed.message else { panic!("{listed:?}") };
    assert!(list.complete && list.sessions.len() == 1 && list.sessions[0].session_id == sid && list.sessions[0].child_running);

    let kill_op = match c.request(Uuid::default(), Message::ReserveOperation(ReserveOperation { kind: OperationKind::Kill })).await.message {
        Message::OperationReserved(o) => o.operation_id,
        other => panic!("{other:?}"),
    };
    let killed = c.request(sid, Message::Kill(Kill { operation_id: kill_op.clone(), signal: Signal::Kill })).await;
    let Message::KillResult(k) = killed.message else { panic!("{killed:?}") };
    assert!(k.delivered);
    let exited = c.event_where(|f| matches!(f.message, Message::ChildExited(_))).await;
    let Message::ChildExited(ex) = exited.message else { unreachable!() };
    assert_eq!(ex.session_incarnation, sp.session_incarnation);
    c.drive(&mut rc, |rc| rc.replica.as_ref().is_some_and(|r| r.exit_info.is_some())).await;

    let again = c.request(sid, Message::Kill(Kill { operation_id: kill_op, signal: Signal::Kill })).await;
    let Message::KillResult(k2) = again.message else { panic!("{again:?}") };
    assert!(k2.delivered, "a replayed Kill returns the stored result");
    let st = c.request(sid, Message::GetSessionState).await;
    let Message::SessionState(st) = st.message else { panic!("{st:?}") };
    assert!(!st.child_running && st.exit_info.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_operation_and_missing_session_are_typed_errors() {
    let (endpoint, _dir) = start_host().await;
    let mut c = Client::open(&endpoint, 3).await;
    let bogus = OperationId { host_instance_id: Uuid([1; 16]), op_seq: 1 };
    let r = c.request(Uuid::default(), Message::Spawn(Spawn { operation_id: bogus, cols: 80, rows: 24, program: "x".into(), args: vec![], env: vec![], cwd: String::new() })).await;
    assert_eq!(error_code(&r), Some(11), "OPERATION_UNKNOWN");
    let r = c.request(Uuid([5; 16]), Message::GetSessionState).await;
    assert_eq!(error_code(&r), Some(8), "SESSION_NOT_FOUND");
    let (spawned, _) = spawn_session(&mut c, "/nonexistent/ferryx-test-binary".into(), vec![], vec![]).await;
    assert_eq!(error_code(&spawned), Some(23), "SPAWN_FAILED");
    let _ = ErrorDetail::SpawnFailed;
}
