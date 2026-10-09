use std::collections::{BTreeMap, BTreeSet};

pub const SEND_WINDOW: u64 = 1024 * 1024;
pub const CHUNK: usize = 64 * 1024;
pub const MAX_INFLIGHT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Written,
    AcceptedPendingWrite,
    AcceptedThenDropped,
    NotDelivered,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Acquire { id: u64, session: u64 },
    Reclaim { id: u64, session: u64, epoch: u64 },
    Release { id: u64, session: u64, epoch: u64 },
    Write { attempt: u64, session: u64, epoch: u64, start: u64, bytes: Vec<u8> },
    GetEpochState { id: u64, session: u64, epoch: u64 },
    Classified { session: u64, epoch: u64, first_offset: u64, bytes: Vec<u8>, statuses: Vec<Status> },
}

type EpochKey = (u64, u64);

#[derive(Debug, Clone)]
struct Segment {
    base: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Cur {
    session: u64,
    epoch: u64,
    accepted: u64,
    committed: u64,
    seg: Segment,
}

#[derive(Debug, Clone)]
struct Finalizing {
    seg: Segment,
    query: Option<u64>,
    retry: bool,
    high_accepted: u64,
    high_committed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKind {
    Acquire,
    Reclaim(u64),
    Probe,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    pub id: u64,
    pub kind: PendingKind,
    pub session: u64,
}

#[derive(Debug, Default)]
pub struct Author {
    pub session: u64,
    pub focused: bool,
    pub typing: bool,
    pub suppressed: bool,
    suppressed_epoch: u64,
    needs_probe: bool,
    needs_reclaim: bool,
    retry_timer: bool,
    send_timer: bool,
    pub pending: Option<Pending>,
    cur: Option<Cur>,
    unassigned: Vec<u8>,
    finalizing: BTreeMap<EpochKey, Finalizing>,
    inflight: BTreeMap<u64, (u64, u64)>,
    pub disowned: BTreeSet<u64>,
    next_id: u64,
    out: Vec<Out>,
}

impl Author {
    pub fn new(session: u64) -> Self {
        Author { session, ..Default::default() }
    }

    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    pub fn current(&self) -> Option<(u64, u64)> {
        self.cur.as_ref().map(|c| (c.session, c.epoch))
    }

    pub fn unassigned_len(&self) -> usize {
        self.unassigned.len()
    }

    pub fn finalizing_epochs(&self) -> Vec<EpochKey> {
        self.finalizing.keys().copied().collect()
    }

    pub fn unaccepted_len(&self) -> u64 {
        self.cur.as_ref().map_or(0, |c| c.seg.base + c.seg.bytes.len() as u64 - c.accepted)
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn holds(&self, session: u64, epoch: u64) -> bool {
        self.cur.as_ref().is_some_and(|c| c.session == session && c.epoch == epoch)
    }

    fn acquire_cond(&self) -> bool {
        self.focused && !self.suppressed && (!self.unassigned.is_empty() || self.typing)
    }

    fn request(&mut self, kind: PendingKind, session: u64) {
        let id = self.id();
        self.pending = Some(Pending { id, kind, session });
        self.out.push(match kind {
            PendingKind::Acquire => Out::Acquire { id, session },
            PendingKind::Reclaim(epoch) => Out::Reclaim { id, session, epoch },
            PendingKind::Probe => Out::Reclaim { id, session, epoch: self.suppressed_epoch },
            PendingKind::Release => Out::Release { id, session, epoch: 0 },
        });
    }

    fn unsuppress(&mut self) {
        self.suppressed = false;
        self.needs_probe = false;
    }

    fn reevaluate(&mut self) {
        let due: Vec<EpochKey> = self.finalizing.iter().filter(|(_, f)| f.query.is_none() && !f.retry).map(|(k, _)| *k).collect();
        for (session, epoch) in due {
            let id = self.id();
            self.finalizing.get_mut(&(session, epoch)).unwrap().query = Some(id);
            self.out.push(Out::GetEpochState { id, session, epoch });
        }
        if self.cur.is_some() && !self.focused && self.unaccepted_len() == 0 && self.inflight.is_empty() {
            self.release_cur();
        }
        if self.pending.is_none() && !self.retry_timer {
            if let (Some(c), true) = (self.cur.as_ref(), self.needs_reclaim) {
                let (session, epoch) = (c.session, c.epoch);
                self.request(PendingKind::Reclaim(epoch), session);
            } else if self.cur.is_none() && self.suppressed && self.needs_probe {
                self.request(PendingKind::Probe, self.session);
            } else if self.cur.is_none() && self.acquire_cond() {
                self.request(PendingKind::Acquire, self.session);
            } else if let Some(session) = self.disowned.first().copied() {
                self.request(PendingKind::Release, session);
            }
        }
        self.send();
    }

    fn release_cur(&mut self) {
        let Some((session, epoch)) = self.current() else { return };
        let id = self.id();
        self.out.push(Out::Release { id, session, epoch });
        self.lose(session, epoch, None, false);
    }

    fn send(&mut self) {
        if self.needs_reclaim || self.send_timer {
            return;
        }
        let Some(c) = self.cur.as_ref() else { return };
        let (session, epoch, accepted) = (c.session, c.epoch, c.accepted);
        let end = c.seg.base + c.seg.bytes.len() as u64;
        let window_end = end.min(accepted + SEND_WINDOW);
        let mut next = self.inflight.values().map(|(_, e)| *e).max().unwrap_or(accepted).max(accepted);
        while self.inflight.len() < MAX_INFLIGHT && next < window_end {
            let stop = window_end.min(next + CHUNK as u64);
            let c = self.cur.as_ref().unwrap();
            let bytes = c.seg.bytes[(next - c.seg.base) as usize..(stop - c.seg.base) as usize].to_vec();
            let attempt = self.id();
            self.inflight.insert(attempt, (next, stop));
            self.out.push(Out::Write { attempt, session, epoch, start: next, bytes });
            next = stop;
        }
    }

    fn lose(&mut self, session: u64, epoch: u64, finals: Option<(u64, u64, u64)>, other: bool) {
        if !self.holds(session, epoch) {
            return;
        }
        let c = self.cur.take().unwrap();
        self.inflight.clear();
        self.needs_reclaim = false;
        self.send_timer = false;
        if self.pending.is_some_and(|p| p.session == session && p.kind == PendingKind::Reclaim(epoch)) {
            self.pending = None;
        }
        self.finalizing.insert((session, epoch), Finalizing { seg: c.seg, query: None, retry: false, high_accepted: c.accepted, high_committed: c.committed });
        if other {
            self.suppressed = true;
            self.suppressed_epoch = epoch;
        }
        if finals.is_some() {
            self.classify(session, epoch, finals);
        }
    }

    fn classify(&mut self, session: u64, epoch: u64, finals: Option<(u64, u64, u64)>) {
        let Some(f) = self.finalizing.remove(&(session, epoch)) else { return };
        let statuses = (0..f.seg.bytes.len() as u64)
            .map(|i| {
                let off = f.seg.base + i;
                match finals {
                    None if off < f.high_committed => Status::Written,
                    None => Status::OutcomeUnknown,
                    Some((_, cm, _)) if off < cm => Status::Written,
                    Some((fa, _, pd)) if off < fa.saturating_sub(pd) => Status::AcceptedPendingWrite,
                    Some((fa, _, _)) if off < fa => Status::AcceptedThenDropped,
                    Some(_) => Status::NotDelivered,
                }
            })
            .collect();
        self.out.push(Out::Classified { session, epoch, first_offset: f.seg.base, bytes: f.seg.bytes, statuses });
    }

    pub fn type_bytes(&mut self, b: &[u8]) {
        self.typing = true;
        self.unsuppress();
        match self.cur.as_mut() {
            Some(c) => c.seg.bytes.extend_from_slice(b),
            None => self.unassigned.extend_from_slice(b),
        }
        self.reevaluate();
    }

    pub fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
        if focused {
            self.unsuppress();
        } else {
            self.typing = false;
        }
        self.reevaluate();
    }

    pub fn rebind(&mut self, session: u64) {
        let old = self.session;
        self.release_cur();
        match self.pending {
            Some(p) if p.kind == PendingKind::Acquire => {
                self.disowned.insert(p.session);
                self.pending = None;
            }
            Some(p) if p.kind == PendingKind::Probe => self.pending = None,
            _ => {}
        }
        if !self.unassigned.is_empty() {
            let bytes = std::mem::take(&mut self.unassigned);
            let statuses = vec![Status::NotDelivered; bytes.len()];
            self.out.push(Out::Classified { session: old, epoch: 0, first_offset: 0, bytes, statuses });
        }
        self.session = session;
        self.typing = false;
        self.unsuppress();
        self.reevaluate();
    }

    pub fn on_granted(&mut self, id: u64, session: u64, epoch: u64) {
        match self.pending {
            Some(p) if p.id == id && p.kind == PendingKind::Acquire => {
                self.pending = None;
                if p.session == session && session == self.session && self.focused && !self.suppressed {
                    let bytes = std::mem::take(&mut self.unassigned);
                    self.cur = Some(Cur { session, epoch, accepted: 0, committed: 0, seg: Segment { base: 0, bytes } });
                    self.inflight.clear();
                    self.disowned.remove(&session);
                } else {
                    self.release_grant(session, epoch);
                }
            }
            _ => self.release_grant(session, epoch),
        }
        self.reevaluate();
    }

    fn release_grant(&mut self, session: u64, epoch: u64) {
        let id = self.id();
        self.out.push(Out::Release { id, session, epoch });
        if !self.cur.as_ref().is_some_and(|c| c.session == session) {
            self.disowned.insert(session);
        }
    }

    pub fn on_released(&mut self, id: u64) {
        if let Some(p) = self.pending.filter(|p| p.id == id && p.kind == PendingKind::Release) {
            self.pending = None;
            self.disowned.remove(&p.session);
        }
        self.reevaluate();
    }

    fn ack(&mut self, session: u64, epoch: u64, accepted: u64, committed: u64) -> bool {
        if let Some(c) = self.cur.as_mut().filter(|c| c.session == session && c.epoch == epoch) {
            let grew = accepted > c.accepted || committed > c.committed;
            c.accepted = c.accepted.max(accepted);
            c.committed = c.committed.max(committed);
            let consumed = (c.accepted - c.seg.base) as usize;
            if consumed > 0 {
                c.seg.bytes.drain(..consumed.min(c.seg.bytes.len()));
                c.seg.base = c.accepted;
            }
            return grew;
        }
        if let Some(f) = self.finalizing.get_mut(&(session, epoch)) {
            f.high_accepted = f.high_accepted.max(accepted);
            f.high_committed = f.high_committed.max(committed);
        }
        false
    }

    pub fn on_input_ack(&mut self, attempt: Option<u64>, session: u64, epoch: u64, accepted: u64, committed: u64) {
        if let (Some(a), true) = (attempt, self.holds(session, epoch)) {
            self.inflight.remove(&a);
        }
        if self.ack(session, epoch, accepted, committed) {
            self.send_timer = false;
            let acc = self.cur.as_ref().map_or(0, |c| c.accepted);
            self.inflight.retain(|_, (_, end)| *end > acc);
        }
        self.reevaluate();
    }

    pub fn on_write_retryable(&mut self, attempt: u64, session: u64, epoch: u64, gap_accepted: Option<u64>) {
        if !self.holds(session, epoch) || !self.inflight.contains_key(&attempt) {
            return;
        }
        if let Some(a) = gap_accepted {
            self.ack(session, epoch, a, 0);
        }
        self.inflight.clear();
        self.send_timer = true;
        self.reevaluate();
    }

    pub fn on_send_timer(&mut self) {
        self.send_timer = false;
        self.reevaluate();
    }

    pub fn on_lease_lost(&mut self, session: u64, epoch: u64, new_epoch_or_current: u64) {
        self.lose(session, epoch, None, new_epoch_or_current != 0);
        self.reevaluate();
    }

    pub fn on_vacated(&mut self, session: u64) {
        if session == self.session {
            self.unsuppress();
        }
        self.reevaluate();
    }

    pub fn on_reclaim_result(&mut self, id: u64, ok: bool, current_epoch: u64, accepted: u64, committed: u64, finals: Option<(u64, u64, u64)>) {
        let Some(p) = self.pending.filter(|p| p.id == id && matches!(p.kind, PendingKind::Reclaim(_) | PendingKind::Probe)) else { return };
        self.pending = None;
        match p.kind {
            PendingKind::Probe if p.session == self.session => {
                self.needs_probe = false;
                if current_epoch == 0 {
                    self.suppressed = false;
                }
            }
            PendingKind::Reclaim(e) if self.holds(p.session, e) => {
                if ok && current_epoch == e {
                    self.needs_reclaim = false;
                    self.inflight.clear();
                    self.ack(p.session, e, accepted, committed);
                } else {
                    self.lose(p.session, e, finals, current_epoch != 0);
                }
            }
            _ => {}
        }
        self.reevaluate();
    }

    pub fn on_reclaim_epoch_unknown(&mut self, id: u64) {
        let Some(p) = self.pending.filter(|p| p.id == id) else { return };
        self.pending = None;
        if let PendingKind::Reclaim(e) = p.kind {
            self.lose(p.session, e, None, true);
        }
        self.reevaluate();
    }

    pub fn on_epoch_state(&mut self, id: u64, session: u64, epoch: u64, fa: u64, cm: u64, pd: u64, fenced: bool) {
        let Some(f) = self.finalizing.get_mut(&(session, epoch)).filter(|f| f.query == Some(id)) else { return };
        f.query = None;
        if fenced {
            self.classify(session, epoch, Some((fa, cm, pd)));
        } else {
            f.retry = true;
            let rid = self.id();
            self.out.push(Out::Release { id: rid, session, epoch });
        }
        self.reevaluate();
    }

    pub fn on_epoch_unknown(&mut self, session: u64, epoch: u64) {
        self.classify(session, epoch, None);
        self.reevaluate();
    }

    /// An Error response or the 5-second timeout. An AcquireLease that ends
    /// without a Grant may still have been granted, so its session is disowned.
    pub fn on_request_failed(&mut self, id: u64) {
        if let Some(p) = self.pending.filter(|p| p.id == id) {
            if p.kind == PendingKind::Acquire {
                self.disowned.insert(p.session);
            }
            self.pending = None;
            self.retry_timer = true;
        }
        for f in self.finalizing.values_mut() {
            if f.query == Some(id) {
                f.query = None;
                f.retry = true;
            }
        }
        self.reevaluate();
    }

    pub fn on_retry_timer(&mut self) {
        self.retry_timer = false;
        for f in self.finalizing.values_mut() {
            f.retry = false;
        }
        self.reevaluate();
    }

    pub fn on_reconnect(&mut self) {
        if let Some(p) = self.pending.filter(|p| p.kind == PendingKind::Acquire) {
            self.disowned.insert(p.session);
        }
        self.pending = None;
        self.retry_timer = false;
        self.inflight.clear();
        if self.cur.is_some() {
            self.needs_reclaim = true;
        }
        if self.suppressed {
            self.needs_probe = true;
        }
        for f in self.finalizing.values_mut() {
            f.query = None;
        }
        self.reevaluate();
    }

    pub fn on_owner_lost(&mut self, session: u64) {
        if let Some((s, e)) = self.current().filter(|(s, _)| *s == session) {
            self.lose(s, e, None, false);
        }
        let lost: Vec<EpochKey> = self.finalizing.keys().copied().filter(|(s, _)| *s == session).collect();
        for (s, e) in lost {
            self.classify(s, e, None);
        }
        self.disowned.remove(&session);
        if self.pending.is_some_and(|p| p.session == session) {
            self.pending = None;
        }
        self.reevaluate();
    }
}
