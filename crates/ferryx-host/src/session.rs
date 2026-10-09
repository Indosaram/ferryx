use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use fxsh::frame::{Frame, FLAG_ERROR, FLAG_EVENT, FLAG_RESPONSE};
use fxsh::messages::*;
use fxsh::types::*;
use fxsh::{state_digest, Codec, Uuid};
use portable_pty::{MasterPty, PtySize};
use session_core::budget::RequestKey;
use session_core::input::{InputLedger, WriteError, MAX_CHUNK, MAX_UNWRITTEN_EPOCHS};
use session_core::replica::normalize_size;
use session_core::resize::{ResizeError, ResizeLease};
use session_core::subscription::{Effect, Subscription};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::host::HostShared;
use crate::outbox::{ConnHandle, SnapItem};
use ferryx_vt::{state_changed, Advance, HostTerminal, DEFAULT_SCROLLBACK_LINES};

pub const MAILBOX_LIMIT: usize = 8 * 1024 * 1024;
pub const SNAPSHOT_CHUNK: usize = 1024 * 1024;
pub const MAX_SUBSCRIBERS: usize = 64;
pub const FLUSH_BACKLOG: usize = 1024 * 1024;
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(4);
pub const EXIT_DRAIN: Duration = Duration::from_millis(250);

pub enum Event {
    Request { conn: ConnHandle, request_id: u64, message: Message },
    ConnClosed { conn_id: u64 },
    PtyOutput(Vec<u8>),
    PtyEof,
    Written(usize),
    WriteFailed,
    ChildExited(ExitInfo),
    DrainDone,
    BudgetAvailable { subscription_id: u64, request_seq: u64, reservation_id: u64 },
    FrameWritten { subscription_id: u64, reservation_id: u64, snapshot_id: u64, last: bool },
    Released { subscription_id: u64, reservation_id: u64, snapshot_id: u64 },
    Deadline { subscription_id: u64, reservation_id: u64, snapshot_id: u64 },
    Flush,
}

#[derive(Default)]
pub struct ByteGate {
    used: Mutex<usize>,
    cv: Condvar,
    closed: AtomicBool,
}

impl ByteGate {
    pub fn acquire(&self, n: usize) {
        let mut u = self.used.lock().expect("gate");
        while *u > 0 && *u + n > MAILBOX_LIMIT && !self.closed.load(Ordering::Acquire) {
            u = self.cv.wait(u).expect("gate");
        }
        *u += n;
    }

    pub fn release(&self, n: usize) {
        let mut u = self.used.lock().expect("gate");
        *u = u.saturating_sub(n);
        self.cv.notify_all();
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.cv.notify_all();
    }
}

pub struct SessionParts {
    pub id: Uuid,
    pub incarnation: Uuid,
    pub creation_op: OperationId,
    pub cols: u16,
    pub rows: u16,
    pub master: Box<dyn MasterPty + Send>,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub tx: UnboundedSender<Event>,
    pub rx: UnboundedReceiver<Event>,
    pub gate: Arc<ByteGate>,
    pub host: Arc<HostShared>,
}

struct Sub {
    s: Subscription,
    conn: ConnHandle,
    deadline: Option<JoinHandle<()>>,
    last_sent_event: u64,
    snapshots_built: u64,
}

pub struct Session {
    id: Uuid,
    incarnation: Uuid,
    creation_op: OperationId,
    host: Arc<HostShared>,
    tx: UnboundedSender<Event>,
    gate: Arc<ByteGate>,
    term: HostTerminal,
    revision: u64,
    ledger: InputLedger,
    resize: ResizeLease,
    subs: BTreeMap<u64, Sub>,
    next_sub_id: u64,
    holders: BTreeMap<[u8; 16], ConnHandle>,
    master: Box<dyn MasterPty + Send>,
    writer_tx: std::sync::mpsc::Sender<Vec<u8>>,
    write_inflight: bool,
    exit: Option<ExitInfo>,
    pending_exit: Option<ExitInfo>,
    flush_scheduled: bool,
}

pub fn start(parts: SessionParts) {
    let SessionParts { id, incarnation, creation_op, cols, rows, master, reader, writer, tx, rx, gate, host } = parts;
    let (cols, rows) = normalize_size(cols, rows);

    let (writer_tx, writer_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let write_events = tx.clone();
    std::thread::spawn(move || {
        let mut writer = writer;
        while let Ok(buf) = writer_rx.recv() {
            match writer.write(&buf) {
                Ok(n) if n > 0 => {
                    let _ = writer.flush();
                    if write_events.send(Event::Written(n)).is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = write_events.send(Event::WriteFailed);
                    break;
                }
            }
        }
    });

    let read_events = tx.clone();
    let read_gate = gate.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    read_gate.acquire(n);
                    if read_events.send(Event::PtyOutput(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
        let _ = read_events.send(Event::PtyEof);
    });

    let session = Session {
        id,
        incarnation,
        creation_op,
        host,
        tx,
        gate,
        term: HostTerminal::new(cols, rows, DEFAULT_SCROLLBACK_LINES),
        revision: 1,
        ledger: InputLedger::new(),
        resize: ResizeLease::new(cols, rows),
        subs: BTreeMap::new(),
        next_sub_id: 1,
        holders: BTreeMap::new(),
        master,
        writer_tx,
        write_inflight: false,
        exit: None,
        pending_exit: None,
        flush_scheduled: false,
    };
    tokio::spawn(session.run(rx));
}

pub fn exit_info_from(status: &portable_pty::ExitStatus) -> ExitInfo {
    #[cfg(unix)]
    let posix_signal = status.signal().and_then(|name| {
        (1..=31u8).find(|n| {
            let p = unsafe { libc::strsignal(*n as i32) };
            !p.is_null() && unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy() == name
        })
    });
    #[cfg(not(unix))]
    let posix_signal = None;
    ExitInfo { exit_code: Some(status.exit_code() as i32), posix_signal }
}

fn empty_delta() -> Delta {
    Delta {
        subscription_id: 0,
        base_revision: 0,
        new_revision: 0,
        size: None,
        cursor: None,
        modes: None,
        palette: None,
        title: None,
        dirty_rows: Vec::new(),
        hyperlinks_added: Vec::new(),
        scrollback_appended: Vec::new(),
        scrollback_evicted_before: None,
        exit_info: None,
        ui_events: Vec::new(),
    }
}

impl Session {
    async fn run(mut self, mut rx: UnboundedReceiver<Event>) {
        self.host.register_alive(self.id);
        while let Some(ev) = rx.recv().await {
            self.on_event(ev);
        }
        self.gate.close();
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Request { conn, request_id, message } => self.on_request(conn, request_id, message),
            Event::ConnClosed { conn_id } => self.on_conn_closed(conn_id),
            Event::PtyOutput(bytes) => {
                self.gate.release(bytes.len());
                let adv = self.term.feed(&bytes);
                self.publish(adv);
            }
            Event::PtyEof => self.finish_child(None),
            Event::Written(n) => {
                self.write_inflight = false;
                if let Some(ack) = self.ledger.on_written(n) {
                    self.send_holder(ack.holder, Message::InputAck(InputAck { client_instance_id: Uuid(ack.holder), epoch: ack.epoch, accepted: ack.accepted, committed: ack.committed }));
                }
                self.pump_write();
            }
            Event::WriteFailed => {
                self.write_inflight = false;
                self.ledger.on_dead();
            }
            Event::ChildExited(info) => {
                self.pending_exit = Some(info);
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(EXIT_DRAIN).await;
                    let _ = tx.send(Event::DrainDone);
                });
            }
            Event::DrainDone => self.finish_child(None),
            Event::BudgetAvailable { subscription_id, request_seq, reservation_id } => {
                let now = self.host.now_ms();
                let mut fx = Vec::new();
                match self.subs.get_mut(&subscription_id) {
                    Some(sub) => sub.s.on_budget_available(request_seq, reservation_id, self.revision, now, &mut fx),
                    None => self.host.return_reservation(reservation_id),
                }
                self.apply(subscription_id, fx, None);
            }
            Event::FrameWritten { subscription_id, reservation_id, snapshot_id, last } => {
                let mut fx = Vec::new();
                if let Some(sub) = self.subs.get_mut(&subscription_id) {
                    sub.s.on_frame_written(reservation_id, snapshot_id, last, &mut fx);
                }
                self.apply(subscription_id, fx, None);
            }
            Event::Released { subscription_id, reservation_id, snapshot_id } => {
                let mut fx = Vec::new();
                if let Some(sub) = self.subs.get_mut(&subscription_id) {
                    sub.s.on_buffer_released(reservation_id, snapshot_id, &mut fx);
                }
                self.apply(subscription_id, fx, None);
            }
            Event::Deadline { subscription_id, reservation_id, snapshot_id } => {
                let mut fx = Vec::new();
                let conn = self.subs.get(&subscription_id).map(|s| s.conn.clone());
                if let Some(sub) = self.subs.get_mut(&subscription_id) {
                    sub.s.on_deadline(reservation_id, snapshot_id, &mut fx);
                }
                let slow = fx.iter().any(|e| matches!(e, Effect::CloseSlowConsumer { .. }));
                self.apply(subscription_id, fx, None);
                if let (true, Some(conn)) = (slow, conn) {
                    conn.outbox.close();
                }
            }
            Event::Flush => {
                self.flush_scheduled = false;
                self.flush_deltas();
            }
        }
    }

    fn frame(&self, request_id: u64, flags: u16, message: Message) -> Vec<u8> {
        Frame { session_id: self.id, request_id, flags, message }.encode()
    }

    fn reply(&self, conn: &ConnHandle, request_id: u64, m: Message) {
        conn.outbox.push_frame(self.frame(request_id, FLAG_RESPONSE, m));
    }

    fn reply_err(&self, conn: &ConnHandle, request_id: u64, d: ErrorDetail, text: &str) {
        conn.outbox.push_frame(self.frame(request_id, FLAG_RESPONSE | FLAG_ERROR, Message::Error(d.to_frame(text))));
    }

    fn event_to(&self, conn: &ConnHandle, m: Message) {
        conn.outbox.push_frame(self.frame(0, FLAG_EVENT, m));
    }

    fn send_holder(&self, holder: [u8; 16], m: Message) {
        if let Some(conn) = self.holders.get(&holder) {
            self.event_to(conn, m);
        }
    }

    fn on_conn_closed(&mut self, conn_id: u64) {
        self.holders.retain(|_, c| c.id != conn_id);
        let ids: Vec<u64> = self.subs.iter().filter(|(_, s)| s.conn.id == conn_id).map(|(id, _)| *id).collect();
        for id in ids {
            let mut fx = Vec::new();
            if let Some(sub) = self.subs.get_mut(&id) {
                sub.s.close(&mut fx);
            }
            self.apply(id, fx, None);
        }
    }

    fn pump_write(&mut self) {
        if self.write_inflight {
            return;
        }
        if let Some(head) = self.ledger.head() {
            if self.writer_tx.send(head.to_vec()).is_ok() {
                self.write_inflight = true;
            }
        }
    }

    fn publish(&mut self, adv: Advance) {
        for reply in adv.vt_replies {
            self.ledger.push_vt_reply(reply);
        }
        self.pump_write();
        if adv.rewritten {
            self.revision += 1;
            let ids: Vec<u64> = self.subs.keys().copied().collect();
            for id in ids {
                let mut fx = Vec::new();
                if let Some(sub) = self.subs.get_mut(&id) {
                    sub.s.resync(&mut fx);
                }
                self.apply(id, fx, None);
            }
            return;
        }
        let Some(mut delta) = adv.delta else { return };
        let changed = state_changed(&delta);
        if !changed && delta.ui_events.is_empty() {
            return;
        }
        let base = self.revision;
        if changed {
            self.revision += 1;
        }
        delta.base_revision = base;
        delta.new_revision = self.revision;
        self.push_delta(delta);
    }

    fn push_delta(&mut self, delta: Delta) {
        let ids: Vec<u64> = self.subs.keys().copied().collect();
        for id in ids {
            let mut fx = Vec::new();
            if let Some(sub) = self.subs.get_mut(&id) {
                sub.s.push_delta(delta.clone(), &mut fx);
            }
            self.apply(id, fx, None);
        }
        self.schedule_flush();
    }

    fn schedule_flush(&mut self) {
        if self.flush_scheduled {
            return;
        }
        self.flush_scheduled = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(FLUSH_INTERVAL).await;
            let _ = tx.send(Event::Flush);
        });
    }

    fn flush_deltas(&mut self) {
        let ids: Vec<u64> = self.subs.keys().copied().collect();
        let mut backlog = false;
        for id in ids {
            let mut fx = Vec::new();
            if let Some(sub) = self.subs.get_mut(&id) {
                if sub.conn.outbox.queued_bytes() < FLUSH_BACKLOG {
                    sub.s.drain_to_wire(&mut fx);
                } else if !sub.s.queue.is_empty() {
                    backlog = true;
                }
            }
            self.apply(id, fx, None);
        }
        if backlog {
            self.schedule_flush();
        }
    }

    fn finish_child(&mut self, info: Option<ExitInfo>) {
        if self.exit.is_some() {
            return;
        }
        let Some(info) = info.or_else(|| self.pending_exit.clone()) else { return };
        self.exit = Some(info.clone());
        self.term.set_exit(info.clone());
        self.ledger.on_dead();
        self.revision += 1;
        let mut delta = empty_delta();
        delta.base_revision = self.revision - 1;
        delta.new_revision = self.revision;
        delta.exit_info = Some(info.clone());
        self.push_delta(delta);
        let table_revision = self.host.mark_exited(self.id, info.clone());
        let msg = Message::ChildExited(ChildExited {
            session_incarnation: self.incarnation,
            exit_code: info.exit_code,
            posix_signal: info.posix_signal,
            table_revision,
            pending_dropped_by_epoch: self.ledger.pending_dropped_by_epoch().into_iter().take(128).collect(),
        });
        for conn in self.host.connections() {
            self.event_to(&conn, msg.clone());
        }
    }

    fn apply(&mut self, sub_id: u64, fx: Vec<Effect>, reply_to: Option<u64>) {
        for e in fx {
            match e {
                Effect::SendAck(ack) => {
                    if let Some(sub) = self.subs.get(&sub_id) {
                        self.reply(&sub.conn, reply_to.unwrap_or(0), Message::SubscribeAck(ack));
                    }
                }
                Effect::SendDelta(d) => {
                    let max_event = d.ui_events.iter().map(|e| e.event_id).max();
                    if let Some(sub) = self.subs.get_mut(&sub_id) {
                        if let Some(m) = max_event {
                            sub.last_sent_event = sub.last_sent_event.max(m);
                        }
                    }
                    if let Some(sub) = self.subs.get(&sub_id) {
                        self.event_to(&sub.conn, Message::Delta(d));
                    }
                }
                Effect::RequestReservation { subscription_id, request_seq } => {
                    self.host.request_reservation(RequestKey { session: self.id.0, subscription_id, request_seq });
                }
                Effect::CancelRequest { subscription_id, request_seq } => {
                    self.host.cancel_reservation(RequestKey { session: self.id.0, subscription_id, request_seq });
                }
                Effect::ReturnReservation { reservation_id } => self.host.return_reservation(reservation_id),
                Effect::BuildSnapshot { subscription_id, snapshot_id, reservation_id } => self.build_snapshot(subscription_id, snapshot_id, reservation_id),
                Effect::DropPending { subscription_id, snapshot_id } => {
                    if let Some(sub) = self.subs.get(&sub_id) {
                        sub.conn.outbox.drop_pending(subscription_id, snapshot_id);
                    }
                }
                Effect::ArmDeadline { subscription_id, reservation_id, snapshot_id, at_ms } => {
                    let wait = Duration::from_millis(at_ms.saturating_sub(self.host.now_ms()));
                    let tx = self.tx.clone();
                    let handle = tokio::spawn(async move {
                        tokio::time::sleep(wait).await;
                        let _ = tx.send(Event::Deadline { subscription_id, reservation_id, snapshot_id });
                    });
                    if let Some(sub) = self.subs.get_mut(&sub_id) {
                        if let Some(old) = sub.deadline.replace(handle) {
                            old.abort();
                        }
                    }
                }
                Effect::CancelDeadline { .. } => {
                    if let Some(sub) = self.subs.get_mut(&sub_id) {
                        if let Some(h) = sub.deadline.take() {
                            h.abort();
                        }
                    }
                }
                Effect::CloseSlowConsumer { subscription_id } => {
                    if let Some(sub) = self.subs.get(&sub_id) {
                        let frame = ErrorDetail::SlowConsumer { subscription_id }.to_frame("subscription closed: slow consumer");
                        sub.conn.outbox.push_frame(self.frame(0, FLAG_EVENT | FLAG_ERROR, Message::Error(frame)));
                    }
                }
            }
        }
        let reap = self.subs.get(&sub_id).is_some_and(|s| s.s.is_closed() && s.s.unreturned.is_empty());
        if reap {
            if let Some(sub) = self.subs.remove(&sub_id) {
                if let Some(h) = sub.deadline {
                    h.abort();
                }
            }
        }
    }

    fn build_snapshot(&mut self, sub_id: u64, snapshot_id: u64, reservation_id: u64) {
        let state = self.term.replica.to_body();
        let digest = state_digest(&state);
        let next = self.term.next_ui_event_id();
        let revision = self.revision;
        let incarnation = self.incarnation;
        let Some(sub) = self.subs.get_mut(&sub_id) else { return };
        let gap = sub.snapshots_built > 0 && next.saturating_sub(1) > sub.last_sent_event;
        sub.snapshots_built += 1;
        sub.last_sent_event = next.saturating_sub(1);
        let body = Body { revision, state_digest: digest, next_ui_event_id: next, ui_event_gap: gap, state };
        let mut enc = Vec::new();
        body.enc(&mut enc);
        let payloads: Vec<SnapshotPayload> = if enc.len() <= SNAPSHOT_CHUNK {
            vec![SnapshotPayload::Full(body)]
        } else {
            let total = enc.len().div_ceil(SNAPSHOT_CHUNK) as u32;
            enc.chunks(SNAPSHOT_CHUNK)
                .enumerate()
                .map(|(i, c)| SnapshotPayload::Chunk { index: i as u32, total, total_len: enc.len() as u32, bytes: fxsh::Bytes(c.to_vec()) })
                .collect()
        };
        let n = payloads.len();
        let session = self.id;
        let sub_conn = sub.conn.clone();
        for (i, payload) in payloads.into_iter().enumerate() {
            let frame = Frame {
                session_id: session,
                request_id: 0,
                flags: FLAG_EVENT,
                message: Message::SnapshotFrame(SnapshotFrame { subscription_id: sub_id, session_incarnation: incarnation, snapshot_id, payload }),
            };
            sub_conn.outbox.push_snap(SnapItem { subscription_id: sub_id, reservation_id, snapshot_id, session: self.tx.clone(), bytes: frame.encode(), last: i + 1 == n });
        }
    }

    fn on_request(&mut self, conn: ConnHandle, rid: u64, msg: Message) {
        match msg {
            Message::AcquireLease(a) => self.acquire(conn, rid, a),
            Message::ReleaseLease(r) => self.release(conn, rid, r),
            Message::Reclaim(r) => self.reclaim(conn, rid, r),
            Message::WriteInput(w) => self.write_input(conn, rid, w),
            Message::GetEpochState(g) => match self.ledger.epoch_state(g.epoch) {
                Some((fa, c, pd, fenced)) => self.reply(&conn, rid, Message::EpochState(EpochState { epoch: g.epoch, final_accepted: fa, committed: c, pending_dropped: pd, fenced })),
                None => self.reply_err(&conn, rid, ErrorDetail::EpochUnknown { epoch: g.epoch }, "epoch unknown"),
            },
            Message::Resize(r) => self.do_resize(conn, rid, r),
            Message::Subscribe(s) => self.subscribe(conn, rid, s),
            Message::Resync(r) => {
                let mut fx = Vec::new();
                if let Some(sub) = self.subs.get_mut(&r.subscription_id) {
                    sub.s.resync(&mut fx);
                }
                self.apply(r.subscription_id, fx, None);
            }
            Message::Unsubscribe(u) => {
                let mut fx = Vec::new();
                if let Some(sub) = self.subs.get_mut(&u.subscription_id) {
                    sub.s.close(&mut fx);
                }
                self.apply(u.subscription_id, fx, None);
            }
            Message::GetSessionState => {
                let body = self.term.replica.to_body();
                let (epoch, accepted, committed) = {
                    let e = self.ledger.current_epoch();
                    let (a, c, _, _) = self.ledger.epoch_state(e).unwrap_or((0, 0, 0, false));
                    (e, a, c)
                };
                let state = SessionState {
                    session_incarnation: self.incarnation,
                    state_revision: self.revision,
                    state_digest: state_digest(&body),
                    input_epoch: epoch,
                    input_accepted: accepted,
                    input_committed: committed,
                    resize_epoch: self.resize.applied.epoch,
                    resize_seq: self.resize.applied.seq,
                    cols: self.term.replica.cols,
                    rows: self.term.replica.rows,
                    child_running: self.exit.is_none(),
                    exit_info: self.exit.clone(),
                    pending_dropped_by_epoch: self.ledger.pending_dropped_by_epoch().into_iter().take(128).collect(),
                    vt_replies_dropped: self.ledger.vt_replies_dropped,
                    creation_operation_id: self.creation_op.clone(),
                };
                self.reply(&conn, rid, Message::SessionState(state));
            }
            other => self.reply_err(&conn, rid, ErrorDetail::UnknownCommand { command: other.command() }, "not a session command"),
        }
    }

    fn acquire(&mut self, conn: ConnHandle, rid: u64, a: AcquireLease) {
        let who = a.client_instance_id.0;
        self.holders.insert(who, conn.clone());
        match a.scope {
            Scope::Input => {
                if !self.ledger.fresh(who, a.lease_request_id) {
                    return;
                }
                match self.ledger.acquire(who) {
                    Ok(g) => {
                        let prior = g.prior.map(|(epoch, final_accepted, committed)| PriorEpoch { epoch, final_accepted, committed });
                        self.reply(&conn, rid, Message::LeaseGranted(LeaseGranted { client_instance_id: a.client_instance_id, lease_request_id: a.lease_request_id, scope: Scope::Input, epoch: g.epoch, prior, resize_applied: None }));
                        if let Some(r) = g.revoked {
                            self.send_holder(r.holder, Message::LeaseRevoked(LeaseRevoked { holder_instance_id: Uuid(r.holder), scope: Scope::Input, revoked_epoch: r.revoked_epoch, new_epoch: r.new_epoch }));
                        }
                    }
                    Err(_) => self.reply_err(&conn, rid, ErrorDetail::LimitExceeded { limit_kind: 8, limit: MAX_UNWRITTEN_EPOCHS as u64 }, "too many epochs with unwritten input"),
                }
            }
            Scope::Resize => {
                if !self.resize.fresh(who, a.lease_request_id) {
                    return;
                }
                let (epoch, applied, revoked) = self.resize.acquire(who);
                self.reply(&conn, rid, Message::LeaseGranted(LeaseGranted { client_instance_id: a.client_instance_id, lease_request_id: a.lease_request_id, scope: Scope::Resize, epoch, prior: None, resize_applied: Some(ResizeApplied { epoch: applied.epoch, seq: applied.seq, cols: applied.cols, rows: applied.rows }) }));
                if let Some((holder, prev_epoch)) = revoked {
                    self.send_holder(holder, Message::LeaseRevoked(LeaseRevoked { holder_instance_id: Uuid(holder), scope: Scope::Resize, revoked_epoch: prev_epoch, new_epoch: epoch }));
                }
            }
        }
    }

    fn release(&mut self, conn: ConnHandle, rid: u64, r: ReleaseLease) {
        let who = r.client_instance_id.0;
        self.holders.insert(who, conn.clone());
        let fresh = match r.scope {
            Scope::Input => self.ledger.fresh(who, r.lease_request_id),
            Scope::Resize => self.resize.fresh(who, r.lease_request_id),
        };
        if !fresh {
            return;
        }
        let (epoch, released, revokees) = match r.scope {
            Scope::Input => {
                let epoch = if r.epoch == 0 { self.ledger.current_epoch() } else { r.epoch };
                let (ok, rv) = self.ledger.release(who, r.epoch);
                (epoch, ok, rv)
            }
            Scope::Resize => {
                let epoch = if r.epoch == 0 { self.resize.current().0 } else { r.epoch };
                let (ok, rv) = self.resize.release(who, r.epoch);
                (epoch, ok, rv)
            }
        };
        self.reply(&conn, rid, Message::LeaseReleased(LeaseReleased { client_instance_id: r.client_instance_id, lease_request_id: r.lease_request_id, scope: r.scope, epoch, released }));
        if released {
            for h in revokees {
                self.send_holder(h, Message::LeaseVacated(LeaseVacated { holder_instance_id: Uuid(h), scope: r.scope, epoch }));
            }
        }
    }

    fn reclaim(&mut self, conn: ConnHandle, rid: u64, r: Reclaim) {
        let who = r.client_instance_id.0;
        self.holders.insert(who, conn.clone());
        let fresh = match r.scope {
            Scope::Input => self.ledger.fresh(who, r.lease_request_id),
            Scope::Resize => self.resize.fresh(who, r.lease_request_id),
        };
        if !fresh {
            return;
        }
        let result = match r.scope {
            Scope::Input => match self.ledger.reclaim(who, r.epoch) {
                Err(()) => return self.reply_err(&conn, rid, ErrorDetail::EpochUnknown { epoch: r.epoch }, "epoch unknown"),
                Ok(Ok((accepted, committed))) => ReclaimResult { client_instance_id: r.client_instance_id, lease_request_id: r.lease_request_id, scope: r.scope, requested_epoch: r.epoch, ok: true, current_epoch: r.epoch, input: Some(InputProgress { accepted, committed }), input_final: None, resize: None },
                Ok(Err((current, final_accepted, committed, pending_dropped))) => ReclaimResult { client_instance_id: r.client_instance_id, lease_request_id: r.lease_request_id, scope: r.scope, requested_epoch: r.epoch, ok: false, current_epoch: current, input: None, input_final: Some(InputFinal { final_accepted, committed, pending_dropped }), resize: None },
            },
            Scope::Resize => {
                let (ok, current, applied) = self.resize.reclaim(who, r.epoch);
                ReclaimResult { client_instance_id: r.client_instance_id, lease_request_id: r.lease_request_id, scope: r.scope, requested_epoch: r.epoch, ok, current_epoch: current, input: None, input_final: None, resize: Some(ResizeApplied { epoch: applied.epoch, seq: applied.seq, cols: applied.cols, rows: applied.rows }) }
            }
        };
        self.reply(&conn, rid, Message::ReclaimResult(result));
    }

    fn write_input(&mut self, conn: ConnHandle, rid: u64, w: WriteInput) {
        let who = w.client_instance_id.0;
        self.holders.insert(who, conn.clone());
        match self.ledger.write(who, w.epoch, w.start, &w.bytes.0, w.crc32c) {
            Ok(ack) => {
                self.reply(&conn, rid, Message::InputAck(InputAck { client_instance_id: w.client_instance_id, epoch: ack.epoch, accepted: ack.accepted, committed: ack.committed }));
                self.pump_write();
            }
            Err((err, revoke)) => {
                let (detail, text) = match err {
                    WriteError::StaleLease { current_epoch, epoch_final_accepted } => (ErrorDetail::StaleLease { scope: Scope::Input, current_epoch, epoch_final_accepted }, "stale input lease"),
                    WriteError::TooLarge => (ErrorDetail::LimitExceeded { limit_kind: 7, limit: MAX_CHUNK as u64 }, "input chunk too large"),
                    WriteError::CorruptFrame => (ErrorDetail::CorruptFrame, "input checksum mismatch"),
                    WriteError::OffsetOverflow => (ErrorDetail::LimitExceeded { limit_kind: 6, limit: u64::MAX }, "input offset overflow"),
                    WriteError::Gap { accepted } => (ErrorDetail::InputGap { accepted }, "input gap"),
                    WriteError::Diverged { epoch, accepted, committed } => (ErrorDetail::InputDiverged { epoch, accepted, committed }, "retransmitted input differs"),
                    WriteError::Unverifiable { epoch, accepted, committed, retained_start } => (ErrorDetail::InputUnverifiable { epoch, accepted, committed, retained_start }, "retransmitted input cannot be verified"),
                    WriteError::Backpressure { accepted } => (ErrorDetail::InputBackpressure { accepted }, "input queue full"),
                };
                self.reply_err(&conn, rid, detail, text);
                if let Some((rv, revokees)) = revoke {
                    self.send_holder(rv.holder, Message::LeaseRevoked(LeaseRevoked { holder_instance_id: Uuid(rv.holder), scope: Scope::Input, revoked_epoch: rv.revoked_epoch, new_epoch: rv.new_epoch }));
                    for h in revokees {
                        self.send_holder(h, Message::LeaseVacated(LeaseVacated { holder_instance_id: Uuid(h), scope: Scope::Input, epoch: rv.revoked_epoch }));
                    }
                }
            }
        }
    }

    fn do_resize(&mut self, conn: ConnHandle, rid: u64, r: Resize) {
        let who = self.resize.current().1.unwrap_or([0; 16]);
        match self.resize.resize(who, r.resize_epoch, r.resize_seq, r.cols, r.rows) {
            Ok(applied) => {
                let _ = self.master.resize(PtySize { rows: applied.rows, cols: applied.cols, pixel_width: 0, pixel_height: 0 });
                let adv = self.term.resize(applied.cols, applied.rows);
                self.publish(adv);
                self.reply(&conn, rid, Message::ResizeAck(ResizeAck { epoch: applied.epoch, seq: applied.seq, cols: applied.cols, rows: applied.rows }));
            }
            Err(ResizeError::StaleLease { current_epoch }) => self.reply_err(&conn, rid, ErrorDetail::StaleLease { scope: Scope::Resize, current_epoch, epoch_final_accepted: 0 }, "stale resize lease"),
            Err(ResizeError::StaleResize(a)) => self.reply_err(&conn, rid, ErrorDetail::StaleResize { applied_epoch: a.epoch, applied_seq: a.seq, cols: a.cols, rows: a.rows }, "stale resize"),
        }
    }

    fn subscribe(&mut self, conn: ConnHandle, rid: u64, s: Subscribe) {
        let existing = self.subs.iter().find(|(_, x)| x.s.subscriber_id == s.subscriber_id && !x.s.is_closed()).map(|(id, x)| (*id, x.s.attach_seq));
        if let Some((old_id, old_attach)) = existing {
            if old_attach >= s.attach_seq {
                return self.reply_err(&conn, rid, ErrorDetail::StaleSubscribe { current_attach_seq: old_attach }, "stale attach_seq");
            }
            let mut fx = Vec::new();
            if let Some(sub) = self.subs.get_mut(&old_id) {
                sub.s.close(&mut fx);
            }
            self.apply(old_id, fx, None);
        }
        if self.subs.values().filter(|x| !x.s.is_closed()).count() >= MAX_SUBSCRIBERS {
            return self.reply_err(&conn, rid, ErrorDetail::LimitExceeded { limit_kind: 4, limit: MAX_SUBSCRIBERS as u64 }, "too many subscribers");
        }
        let id = self.next_sub_id;
        self.next_sub_id += 1;
        let known = match (s.client_known_revision, s.client_known_incarnation) {
            (Some(r), Some(i)) => Some((r, i)),
            _ => None,
        };
        self.subs.insert(id, Sub { s: Subscription::new(id, s.subscriber_id, s.attach_seq), conn, deadline: None, last_sent_event: 0, snapshots_built: 0 });
        let mut fx = Vec::new();
        let revision = self.revision;
        let incarnation = self.incarnation;
        if let Some(sub) = self.subs.get_mut(&id) {
            sub.s.start_with_ack(incarnation, revision, known, &mut fx);
        }
        self.apply(id, fx, Some(rid));
    }
}
