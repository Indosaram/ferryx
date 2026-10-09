use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use fxsh::frame::{Frame, FLAG_ERROR, FLAG_RESPONSE};
use fxsh::messages::*;
use fxsh::types::*;
use fxsh::{Codec, Uuid};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use session_core::budget::{Allocator, Grant, RequestKey};
use session_core::operations::{request_hash, Decision, OperationTable};
use session_core::replica::{normalize_size, S_MAX};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::outbox::ConnHandle;
use crate::session::{exit_info_from, start, ByteGate, Event, SessionParts};

pub const SNAPSHOT_BUDGET: u64 = 512 * 1024 * 1024;
pub const RESERVATION_SIZE: u64 = (S_MAX + 1024 * 1024) as u64;
pub const MAX_SESSIONS: usize = 1024;
pub const MAX_SPAWN_ARGS: usize = 1024 * 1024;

struct Entry {
    pid: u32,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    incarnation: Uuid,
    creation_op: OperationId,
    created_table_revision: u64,
    tx: UnboundedSender<Event>,
    exit: Option<ExitInfo>,
    ended_ms: Option<u64>,
}

struct Inner {
    sessions: BTreeMap<[u8; 16], Entry>,
    table_revision: u64,
    ops: OperationTable,
    alloc: Allocator,
    conns: BTreeMap<u64, ConnHandle>,
}

pub struct HostShared {
    pub instance_id: Uuid,
    started: Instant,
    next_conn: AtomicU64,
    inner: Mutex<Inner>,
}

fn random_uuid() -> Uuid {
    Uuid(*uuid::Uuid::new_v4().as_bytes())
}

impl HostShared {
    pub fn new(instance_id: Uuid) -> Arc<Self> {
        Arc::new(HostShared {
            instance_id,
            started: Instant::now(),
            next_conn: AtomicU64::new(1),
            inner: Mutex::new(Inner {
                sessions: BTreeMap::new(),
                table_revision: 0,
                ops: OperationTable::new(instance_id),
                alloc: Allocator::new(SNAPSHOT_BUDGET, RESERVATION_SIZE),
                conns: BTreeMap::new(),
            }),
        })
    }

    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn next_conn_id(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    pub fn register_conn(&self, conn: ConnHandle) {
        self.inner.lock().expect("host").conns.insert(conn.id, conn);
    }

    pub fn connections(&self) -> Vec<ConnHandle> {
        self.inner.lock().expect("host").conns.values().cloned().collect()
    }

    pub fn conn_closed(&self, conn_id: u64) {
        let txs: Vec<UnboundedSender<Event>> = {
            let mut g = self.inner.lock().expect("host");
            g.conns.remove(&conn_id);
            g.sessions.values().map(|e| e.tx.clone()).collect()
        };
        for tx in txs {
            let _ = tx.send(Event::ConnClosed { conn_id });
        }
    }

    pub fn register_alive(&self, _id: Uuid) {}

    pub fn mark_exited(&self, id: Uuid, info: ExitInfo) -> u64 {
        let now = self.now_ms();
        let mut g = self.inner.lock().expect("host");
        g.table_revision += 1;
        let rev = g.table_revision;
        if let Some(e) = g.sessions.get_mut(&id.0) {
            e.exit = Some(info);
            e.ended_ms = Some(now);
        }
        rev
    }

    pub fn request_reservation(&self, key: RequestKey) {
        let grants = self.inner.lock().expect("host").alloc.request(key);
        self.route(grants);
    }

    pub fn cancel_reservation(&self, key: RequestKey) {
        let grants = self.inner.lock().expect("host").alloc.cancel(key);
        self.route(grants);
    }

    pub fn return_reservation(&self, reservation_id: u64) {
        let grants = self.inner.lock().expect("host").alloc.give_back(reservation_id);
        self.route(grants);
    }

    fn route(&self, grants: Vec<Grant>) {
        for g in grants {
            let tx = self.inner.lock().expect("host").sessions.get(&g.key.session).map(|e| e.tx.clone());
            match tx {
                Some(tx) => {
                    let _ = tx.send(Event::BudgetAvailable { subscription_id: g.key.subscription_id, request_seq: g.key.request_seq, reservation_id: g.reservation_id });
                }
                None => self.return_reservation(g.reservation_id),
            }
        }
    }

    pub fn session_tx(&self, id: &Uuid) -> Option<UnboundedSender<Event>> {
        self.inner.lock().expect("host").sessions.get(&id.0).map(|e| e.tx.clone())
    }

    fn reply(conn: &ConnHandle, session: Uuid, rid: u64, m: Message) {
        conn.outbox.push_frame(Frame { session_id: session, request_id: rid, flags: FLAG_RESPONSE, message: m }.encode());
    }

    fn reply_err(conn: &ConnHandle, session: Uuid, rid: u64, d: ErrorDetail, text: &str) {
        conn.outbox.push_frame(Frame { session_id: session, request_id: rid, flags: FLAG_RESPONSE | FLAG_ERROR, message: Message::Error(d.to_frame(text)) }.encode());
    }

    pub fn handle(self: &Arc<Self>, conn: &ConnHandle, frame: Frame) {
        let Frame { session_id, request_id, message, .. } = frame;
        match message {
            Message::ReserveOperation(r) => {
                let now = self.now_ms();
                let id = self.inner.lock().expect("host").ops.reserve(r.kind, now);
                Self::reply(conn, session_id, request_id, Message::OperationReserved(OperationReserved { operation_id: id }));
            }
            Message::Spawn(s) => self.spawn(conn, session_id, request_id, s),
            Message::Kill(k) => self.kill(conn, session_id, request_id, k),
            Message::ListSessions(l) => {
                let g = self.inner.lock().expect("host");
                let sessions = g
                    .sessions
                    .iter()
                    .map(|(id, e)| SessionEntry { session_id: Uuid(*id), session_incarnation: e.incarnation, created_table_revision: e.created_table_revision, creation_operation_id: e.creation_op.clone(), child_running: e.exit.is_none(), exit_info: e.exit.clone() })
                    .collect();
                let list = SessionList { request_token: l.request_token, owner_instance_id: self.instance_id, table_revision: g.table_revision, complete: true, sessions };
                drop(g);
                Self::reply(conn, session_id, request_id, Message::SessionList(list));
            }
            other => match self.session_tx(&session_id) {
                Some(tx) => {
                    let _ = tx.send(Event::Request { conn: conn.clone(), request_id, message: other });
                }
                None => Self::reply_err(conn, session_id, request_id, ErrorDetail::SessionNotFound, "session not found"),
            },
        }
    }

    fn decide(&self, id: &OperationId, kind: OperationKind, command: u16, session: &Uuid, payload: &[u8]) -> Decision {
        let hash = request_hash(command, &session.0, payload);
        self.inner.lock().expect("host").ops.decide(id, kind, hash)
    }

    fn reject(conn: &ConnHandle, session: Uuid, rid: u64, d: Decision, id: &OperationId) -> bool {
        let detail = match d {
            Decision::Execute => return false,
            Decision::Replay(stored) => {
                conn.outbox.push_frame(stored_frame(&stored, session, rid));
                return true;
            }
            Decision::Unknown => ErrorDetail::OperationUnknown(id.clone()),
            Decision::Expired => ErrorDetail::OperationExpired(id.clone()),
            Decision::Conflict => ErrorDetail::OperationConflict(id.clone()),
        };
        Self::reply_err(conn, session, rid, detail, "operation rejected");
        true
    }

    fn spawn(self: &Arc<Self>, conn: &ConnHandle, session_id: Uuid, rid: u64, s: Spawn) {
        let mut payload = Vec::new();
        s.enc(&mut payload);
        let decision = self.decide(&s.operation_id, OperationKind::Spawn, 0x03, &session_id, &payload);
        if let Decision::Replay(stored) = &decision {
            let replayed = replay_spawn(stored);
            conn.outbox.push_frame(replayed.map_or_else(|| stored_frame(stored, session_id, rid), |frame| Frame { session_id: frame.0, request_id: rid, flags: FLAG_RESPONSE, message: Message::SpawnResult(frame.1) }.encode()));
            return;
        }
        if Self::reject(conn, session_id, rid, decision, &s.operation_id) {
            return;
        }
        let hash = request_hash(0x03, &session_id.0, &payload);
        let args_len: usize = s.program.len() + s.cwd.len() + s.args.iter().map(String::len).sum::<usize>() + s.env.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
        let outcome: Result<(Uuid, SpawnResult), (ErrorDetail, String)> = if args_len > MAX_SPAWN_ARGS {
            Err((ErrorDetail::LimitExceeded { limit_kind: 2, limit: MAX_SPAWN_ARGS as u64 }, "spawn arguments too large".into()))
        } else if self.inner.lock().expect("host").sessions.len() >= MAX_SESSIONS {
            Err((ErrorDetail::LimitExceeded { limit_kind: 3, limit: MAX_SESSIONS as u64 }, "too many sessions".into()))
        } else {
            self.start_session(&s)
        };
        let now = self.now_ms();
        match outcome {
            Ok((sid, result)) => {
                let frame = Frame { session_id: sid, request_id: rid, flags: FLAG_RESPONSE, message: Message::SpawnResult(result.clone()) };
                let mut stored = result.clone();
                stored.created = false;
                let stored_frame = Frame { session_id: sid, request_id: 0, flags: FLAG_RESPONSE, message: Message::SpawnResult(stored) }.encode();
                self.inner.lock().expect("host").ops.complete(&s.operation_id, hash, stored_frame, Some(sid.0), now);
                conn.outbox.push_frame(frame.encode());
            }
            Err((detail, text)) => {
                let frame = Frame { session_id, request_id: 0, flags: FLAG_RESPONSE | FLAG_ERROR, message: Message::Error(detail.to_frame(text.clone())) }.encode();
                self.inner.lock().expect("host").ops.complete(&s.operation_id, hash, frame, None, now);
                Self::reply_err(conn, session_id, rid, detail, &text);
            }
        }
    }

    fn start_session(self: &Arc<Self>, s: &Spawn) -> Result<(Uuid, SpawnResult), (ErrorDetail, String)> {
        let (cols, rows) = normalize_size(s.cols, s.rows);
        let fail = |what: &str, e: &dyn std::fmt::Display| (ErrorDetail::SpawnFailed, format!("{what}: {e}"));
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| fail("open pty", &e))?;
        let mut cmd = CommandBuilder::new(&s.program);
        cmd.args(&s.args);
        for (k, v) in &s.env {
            cmd.env(k, v);
        }
        if !s.cwd.is_empty() {
            cmd.cwd(&s.cwd);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| fail("spawn", &e))?;
        drop(pair.slave);
        let pid = child.process_id().unwrap_or(0);
        let reader = pair.master.try_clone_reader().map_err(|e| fail("pty reader", &e))?;
        let writer = pair.master.take_writer().map_err(|e| fail("pty writer", &e))?;
        let id = random_uuid();
        let incarnation = random_uuid();
        let (tx, rx) = unbounded_channel();
        let revision = {
            let mut g = self.inner.lock().expect("host");
            g.table_revision += 1;
            let rev = g.table_revision;
            g.sessions.insert(id.0, Entry { pid, killer: child.clone_killer(), incarnation, creation_op: s.operation_id.clone(), created_table_revision: rev, tx: tx.clone(), exit: None, ended_ms: None });
            rev
        };
        let exit_tx = tx.clone();
        std::thread::spawn(move || {
            let info = match child.wait() {
                Ok(status) => exit_info_from(&status),
                Err(_) => ExitInfo { exit_code: None, posix_signal: None },
            };
            let _ = exit_tx.send(Event::ChildExited(info));
        });
        start(SessionParts { id, incarnation, creation_op: s.operation_id.clone(), cols, rows, master: pair.master, reader, writer, tx, rx, gate: Arc::new(ByteGate::default()), host: self.clone() });
        Ok((id, SpawnResult { operation_id: s.operation_id.clone(), session_id: id, session_incarnation: incarnation, pid, created: true, created_table_revision: revision }))
    }

    fn kill(&self, conn: &ConnHandle, session_id: Uuid, rid: u64, k: Kill) {
        let mut payload = Vec::new();
        k.enc(&mut payload);
        let decision = self.decide(&k.operation_id, OperationKind::Kill, 0x1C, &session_id, &payload);
        if Self::reject(conn, session_id, rid, decision, &k.operation_id) {
            return;
        }
        let hash = request_hash(0x1C, &session_id.0, &payload);
        let delivered = {
            let mut g = self.inner.lock().expect("host");
            match g.sessions.get_mut(&session_id.0) {
                Some(e) if e.exit.is_none() => crate::signal::deliver(e.pid, e.killer.as_mut(), k.signal),
                _ => false,
            }
        };
        let result = Message::KillResult(KillResult { operation_id: k.operation_id.clone(), delivered });
        let stored = Frame { session_id, request_id: 0, flags: FLAG_RESPONSE, message: result.clone() }.encode();
        let now = self.now_ms();
        self.inner.lock().expect("host").ops.complete(&k.operation_id, hash, stored, None, now);
        Self::reply(conn, session_id, rid, result);
    }
}

fn stored_frame(stored: &[u8], session: Uuid, rid: u64) -> Vec<u8> {
    let mut out = stored.to_vec();
    out[12..28].copy_from_slice(&session.0);
    out[28..36].copy_from_slice(&rid.to_be_bytes());
    out
}

fn replay_spawn(stored: &[u8]) -> Option<(Uuid, SpawnResult)> {
    match fxsh::decode_complete_frame(stored) {
        Ok(fxsh::Decoded::Frame { frame, .. }) => match frame.message {
            Message::SpawnResult(r) => Some((frame.session_id, r)),
            _ => None,
        },
        _ => None,
    }
}
