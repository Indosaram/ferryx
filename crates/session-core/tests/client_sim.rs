mod common;

use std::collections::VecDeque;

use fxsh::types::{Body, Cell, Delta, RowData, SnapshotFrame, SnapshotPayload, UiEvent, UiEventKind};
use fxsh::{Bytes, Codec, Uuid};
use proptest::prelude::*;
use session_core::client::{ClientOut, ReplicaClient};
use session_core::replica::{diff, Replica};

const INCARNATION: Uuid = Uuid([5; 16]);

#[derive(Debug, Clone)]
enum Frame {
    Ack { sub: u64, follows: bool, revision: u64 },
    Snap(SnapshotFrame),
    Delta(Delta),
}

struct Host {
    replica: Replica,
    revision: u64,
    next_line: u64,
    next_event: u64,
    last_event_sent: u64,
    sub: u64,
    next_snapshot: u64,
    chunk: usize,
    wire: VecDeque<Frame>,
    stale: Vec<Frame>,
}

impl Host {
    fn new(chunk: usize) -> Self {
        Host { replica: Replica::new(6, 4), revision: 0, next_line: 1, next_event: 1, last_event_sent: 0, sub: 0, next_snapshot: 0, chunk, wire: VecDeque::new(), stale: Vec::new() }
    }

    fn mutate(&mut self, op: u8, tag: u32) {
        let prev = self.replica.clone();
        let r = &mut self.replica;
        match op % 5 {
            0 => r.screen[(tag % r.rows as u32) as usize].cells[0].codepoint = 'a' as u32 + tag % 26,
            1 => {
                let top = r.screen.remove(0);
                r.scrollback.push_back((self.next_line, top));
                self.next_line += 1;
                r.screen.push(RowData { wrapped: false, cells: vec![Cell::BLANK; r.cols as usize] });
            }
            2 => r.title = format!("t{tag}"),
            3 => r.cursor.col = (tag % r.cols as u32) as u16,
            _ => {
                r.scrollback.pop_front();
            }
        }
        let bell = tag % 3 == 0;
        if prev == self.replica && !bell {
            return;
        }
        let mut d = diff(&prev, &self.replica, self.sub, self.revision, self.revision + 1).unwrap();
        self.revision += 1;
        if bell {
            d.ui_events.push(UiEvent { event_id: self.next_event, kind: UiEventKind::Bell });
            self.last_event_sent = self.next_event;
            self.next_event += 1;
        }
        self.wire.push_back(Frame::Delta(d));
    }

    fn snapshot(&mut self, first: bool) {
        self.next_snapshot += 1;
        let state = self.replica.to_body();
        let gap = !first && self.last_event_sent + 1 < self.next_event;
        let body = Body { revision: self.revision, state_digest: fxsh::state_digest(&state), next_ui_event_id: self.next_event, ui_event_gap: gap, state };
        let frame = |payload| Frame::Snap(SnapshotFrame { subscription_id: self.sub, session_incarnation: INCARNATION, snapshot_id: self.next_snapshot, payload });
        if self.chunk == 0 {
            let f = frame(SnapshotPayload::Full(body));
            self.wire.push_back(f);
        } else {
            let mut bytes = Vec::new();
            body.enc(&mut bytes);
            let total = bytes.len().div_ceil(self.chunk) as u32;
            let frames: Vec<Frame> = bytes
                .chunks(self.chunk)
                .enumerate()
                .map(|(i, p)| frame(SnapshotPayload::Chunk { index: i as u32, total, total_len: bytes.len() as u32, bytes: Bytes(p.to_vec()) }))
                .collect();
            self.wire.extend(frames);
        }
        self.last_event_sent = self.next_event - 1;
    }

    fn subscribe(&mut self, known: Option<u64>) {
        self.stale.extend(self.wire.drain(..));
        self.sub += 1;
        if known == Some(self.revision) {
            self.wire.push_back(Frame::Ack { sub: self.sub, follows: false, revision: self.revision });
        } else {
            self.wire.push_back(Frame::Ack { sub: self.sub, follows: true, revision: 0 });
            self.snapshot(known.is_none());
        }
    }
}

struct Client {
    c: ReplicaClient,
    events: Vec<u64>,
    resyncs: u32,
}

impl Client {
    fn new() -> Self {
        Client { c: ReplicaClient::new(INCARNATION), events: Vec::new(), resyncs: 0 }
    }

    fn receive(&mut self, h: &mut Host, f: Frame) -> Result<(), TestCaseError> {
        let out = match f {
            Frame::Ack { sub, follows, revision } => {
                if sub == h.sub {
                    self.c.adopt(sub, follows, revision);
                }
                None
            }
            Frame::Snap(s) => self.c.on_snapshot(&s),
            Frame::Delta(d) => self.c.on_delta(&d),
        };
        match out {
            Some(ClientOut::Resync) => {
                self.resyncs += 1;
                prop_assert!(self.resyncs < 10_000, "resync storm");
                h.snapshot(false);
            }
            Some(ClientOut::DigestMismatch) => return Err(TestCaseError::fail("digest mismatch on an uncorrupted snapshot")),
            Some(ClientOut::Applied { events, .. }) => self.events.extend(events.iter().map(|e| e.event_id)),
            None => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum Step {
    Mutate(u8, u32),
    Resubscribe,
    Deliver,
    DeliverStale(usize),
    Corrupt,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        5 => (any::<u8>(), any::<u32>()).prop_map(|(o, t)| Step::Mutate(o, t)),
        1 => Just(Step::Resubscribe),
        8 => Just(Step::Deliver),
        2 => any::<usize>().prop_map(Step::DeliverStale),
        1 => Just(Step::Corrupt),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: common::cases(3000), .. ProptestConfig::default() })]

    #[test]
    fn client_replica_converges(steps in prop::collection::vec(step(), 0..250), chunk in prop_oneof![Just(0usize), 8usize..512]) {
        let mut h = Host::new(chunk);
        let mut cl = Client::new();
        h.subscribe(None);
        for st in steps {
            match st {
                Step::Mutate(o, t) => h.mutate(o, t),
                Step::Resubscribe => {
                    let known = cl.c.replica.as_ref().map(|_| cl.c.local_revision);
                    h.subscribe(known);
                }
                Step::Deliver => {
                    if let Some(f) = h.wire.pop_front() {
                        cl.receive(&mut h, f)?;
                    }
                }
                Step::DeliverStale(k) => {
                    if !h.stale.is_empty() {
                        let f = h.stale.swap_remove(k % h.stale.len());
                        cl.receive(&mut h, f)?;
                    }
                }
                Step::Corrupt => {
                    if let Some(Frame::Snap(SnapshotFrame { payload: SnapshotPayload::Chunk { index, .. }, .. })) = h.wire.iter_mut().find(|f| matches!(f, Frame::Snap(SnapshotFrame { payload: SnapshotPayload::Chunk { .. }, .. }))) {
                        *index += 1;
                    }
                }
            }
        }
        while let Some(f) = h.wire.pop_front() {
            cl.receive(&mut h, f)?;
        }
        let replica = cl.c.replica.as_ref().ok_or_else(|| TestCaseError::fail("client has no replica"))?;
        prop_assert_eq!(cl.c.local_revision, h.revision);
        prop_assert_eq!(fxsh::state_digest(&replica.to_body()), fxsh::state_digest(&h.replica.to_body()));
        prop_assert!(cl.events.windows(2).all(|w| w[0] < w[1]), "ui events duplicated or reordered: {:?}", cl.events);
        prop_assert!(cl.events.iter().all(|e| *e < h.next_event));
    }
}

#[test]
fn stale_subscription_chunks_do_not_disturb_assembly() {
    let mut h = Host::new(16);
    let mut cl = Client::new();
    h.subscribe(None);
    h.mutate(0, 7);
    h.subscribe(None);
    let ack = h.wire.pop_front().unwrap();
    cl.receive(&mut h, ack).unwrap();
    for f in std::mem::take(&mut h.stale) {
        cl.receive(&mut h, f).unwrap();
    }
    while let Some(f) = h.wire.pop_front() {
        cl.receive(&mut h, f).unwrap();
    }
    assert_eq!(cl.resyncs, 0);
    assert_eq!(cl.c.local_revision, h.revision);
    assert_eq!(fxsh::state_digest(&cl.c.replica.unwrap().to_body()), fxsh::state_digest(&h.replica.to_body()));
}

#[test]
fn broken_chunk_stream_requests_exactly_one_resync() {
    let mut h = Host::new(8);
    for t in 0..20 {
        h.mutate(1, t);
    }
    let mut cl = Client::new();
    h.subscribe(None);
    let ack = h.wire.pop_front().unwrap();
    cl.receive(&mut h, ack).unwrap();
    let chunks = h.wire.len();
    assert!(chunks > 3);
    let first = h.wire.pop_front().unwrap();
    cl.receive(&mut h, first.clone()).unwrap();
    cl.receive(&mut h, first).unwrap();
    assert_eq!(cl.resyncs, 1, "duplicate chunk index must discard the assembly");
    h.mutate(0, 3);
    while let Some(f) = h.wire.pop_front() {
        cl.receive(&mut h, f).unwrap();
    }
    assert_eq!(cl.resyncs, 1, "remaining chunks of a discarded snapshot are ignored");
    assert_eq!(cl.c.local_revision, h.revision);
    assert_eq!(fxsh::state_digest(&cl.c.replica.unwrap().to_body()), fxsh::state_digest(&h.replica.to_body()));
}

#[test]
fn deltas_in_flight_behind_a_discarded_snapshot_do_not_trigger_more_resyncs() {
    let mut h = Host::new(8);
    for t in 0..10 {
        h.mutate(1, t);
    }
    let mut cl = Client::new();
    h.subscribe(None);
    h.mutate(0, 1);
    while let Some(f) = h.wire.pop_front() {
        cl.receive(&mut h, f).unwrap();
    }
    assert!(cl.c.replica.is_some());
    h.mutate(0, 2);
    h.wire.clear();
    h.snapshot(false);
    h.mutate(0, 3);
    let first = h.wire.pop_front().unwrap();
    cl.receive(&mut h, first.clone()).unwrap();
    cl.receive(&mut h, first).unwrap();
    assert_eq!(cl.resyncs, 1);
    while let Some(f) = h.wire.pop_front() {
        cl.receive(&mut h, f).unwrap();
    }
    assert_eq!(cl.resyncs, 1, "a Delta that predates the requested snapshot is buffered, not answered with another Resync");
    assert_eq!(cl.c.local_revision, h.revision);
    assert_eq!(fxsh::state_digest(&cl.c.replica.unwrap().to_body()), fxsh::state_digest(&h.replica.to_body()));
}
