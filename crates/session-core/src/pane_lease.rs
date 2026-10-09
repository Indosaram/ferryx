use std::collections::BTreeSet;

use crate::replica::normalize_size;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKind {
    Acquire,
    Reclaim,
    Probe,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pending {
    pub id: u64,
    pub kind: PendingKind,
    pub session: u64,
    pub generation: u64,
    pub requested_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    pub session: u64,
    pub generation: u64,
    pub epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Acquire { id: u64, session: u64 },
    Reclaim { id: u64, session: u64, epoch: u64 },
    Release { id: u64, session: u64, epoch: u64 },
    Resize { session: u64, epoch: u64, seq: u64, cols: u16, rows: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Policy {
    pub bound_session: Option<u64>,
    pub generation: u64,
    pub visible_focused: bool,
    pub visible_applied_generation: Option<u64>,
    pub gpu_ready: bool,
    pub tombstoned: bool,
}

#[derive(Debug, Default)]
pub struct PaneLease {
    pub policy: Policy,
    pub lease: Option<Lease>,
    pub needs_reclaim: bool,
    pub needs_probe: bool,
    pub pending: Option<Pending>,
    pub retry_timer: bool,
    pub suppressed: bool,
    pub suppressed_epoch: u64,
    pub next_seq: u64,
    pub latest_sent: Option<(u64, u16, u16)>,
    pub pty_acked: Option<u64>,
    pub pty_desired: Option<(u16, u16)>,
    pub disowned: BTreeSet<u64>,
    next_request_id: u64,
    out: Vec<Out>,
}

impl PaneLease {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    fn eligible(&self) -> bool {
        let p = &self.policy;
        !p.tombstoned && p.bound_session.is_some() && p.visible_focused && p.gpu_ready && !self.suppressed
            && p.visible_applied_generation == Some(p.generation)
    }

    fn new_id(&mut self) -> u64 {
        self.next_request_id += 1;
        self.next_request_id
    }

    fn invalidate(&mut self) {
        if let (Some(l), Some(p)) = (self.lease, self.pending) {
            if p.kind == PendingKind::Reclaim && p.requested_epoch == l.epoch {
                self.pending = None;
            }
        }
        self.lease = None;
        self.needs_reclaim = false;
        self.latest_sent = None;
        self.pty_acked = None;
    }

    fn suppress(&mut self, epoch: u64) {
        self.suppressed = true;
        self.suppressed_epoch = epoch;
    }

    fn unsuppress(&mut self) {
        self.suppressed = false;
        self.needs_probe = false;
    }

    fn relinquish(&mut self) {
        if let Some(l) = self.lease {
            let id = self.new_id();
            self.out.push(Out::Release { id, session: l.session, epoch: l.epoch });
            self.disowned.insert(l.session);
            self.invalidate();
        }
        if let Some(p) = self.pending.filter(|p| p.kind == PendingKind::Acquire) {
            self.disowned.insert(p.session);
            self.pending = None;
        }
    }

    fn send(&mut self, kind: PendingKind, session: u64, generation: u64, requested_epoch: u64) {
        let id = self.new_id();
        self.pending = Some(Pending { id, kind, session, generation, requested_epoch });
        self.out.push(match kind {
            PendingKind::Acquire => Out::Acquire { id, session },
            PendingKind::Reclaim | PendingKind::Probe => Out::Reclaim { id, session, epoch: requested_epoch },
            PendingKind::Release => Out::Release { id, session, epoch: 0 },
        });
    }

    fn reevaluate(&mut self) {
        if !self.eligible() && self.lease.is_some() {
            self.relinquish();
        }
        if self.pending.is_some() || self.retry_timer {
            return;
        }
        let generation = self.policy.generation;
        if let (Some(l), true) = (self.lease, self.needs_reclaim) {
            self.send(PendingKind::Reclaim, l.session, l.generation, l.epoch);
        } else if let (None, true, true, Some(session)) = (self.lease, self.suppressed, self.needs_probe, self.policy.bound_session) {
            self.send(PendingKind::Probe, session, generation, self.suppressed_epoch);
        } else if self.eligible() && self.lease.is_none() {
            self.send(PendingKind::Acquire, self.policy.bound_session.unwrap(), generation, 0);
        } else if let Some(session) = self.disowned.first().copied() {
            debug_assert!(self.lease.map_or(true, |l| l.session != session));
            self.send(PendingKind::Release, session, generation, 0);
        }
    }

    fn pty_sync(&mut self) {
        let Some(l) = self.lease else { return };
        let Some((cols, rows)) = self.pty_desired else { return };
        if self.needs_reclaim || !self.eligible() || l.generation != self.policy.generation {
            return;
        }
        if self.latest_sent.is_some_and(|(_, c, r)| (c, r) == (cols, rows)) {
            return;
        }
        self.next_seq += 1;
        let seq = self.next_seq;
        self.latest_sent = Some((seq, cols, rows));
        self.out.push(Out::Resize { session: l.session, epoch: l.epoch, seq, cols, rows });
    }

    pub fn set_policy(&mut self, p: Policy, user_intent: bool) {
        let rebound = p.bound_session != self.policy.bound_session || p.generation != self.policy.generation;
        self.policy = p;
        if user_intent {
            self.unsuppress();
        }
        if rebound {
            self.relinquish();
        }
        self.reevaluate();
    }

    pub fn on_visible_applied(&mut self, cols: u16, rows: u16) {
        self.policy.visible_applied_generation = Some(self.policy.generation);
        self.pty_desired = Some(normalize_size(cols, rows));
        self.pty_sync();
        self.reevaluate();
    }

    pub fn on_retry_timer(&mut self) {
        self.retry_timer = false;
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
        self.reevaluate();
    }

    fn release_grant(&mut self, session: u64, epoch: u64) {
        let id = self.new_id();
        self.out.push(Out::Release { id, session, epoch });
        if !self.lease.is_some_and(|l| l.session == session) {
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

    pub fn on_reconnect(&mut self) {
        if let Some(p) = self.pending.filter(|p| p.kind == PendingKind::Acquire) {
            self.disowned.insert(p.session);
        }
        self.pending = None;
        self.retry_timer = false;
        if self.lease.is_some() {
            self.needs_reclaim = true;
        }
        if self.suppressed {
            self.needs_probe = true;
        }
        self.reevaluate();
    }

    pub fn on_granted(&mut self, id: u64, session: u64, epoch: u64, applied_seq: u64) {
        match self.pending {
            Some(p) if p.kind == PendingKind::Acquire && p.id == id => {
                self.pending = None;
                if p.session == session && self.policy.bound_session == Some(p.session) && self.policy.generation == p.generation && self.eligible() {
                    self.lease = Some(Lease { session, generation: p.generation, epoch });
                    self.disowned.remove(&session);
                    self.needs_reclaim = false;
                    self.unsuppress();
                    self.next_seq = applied_seq;
                    self.latest_sent = None;
                    self.pty_acked = None;
                    self.pty_sync();
                } else {
                    self.release_grant(session, epoch);
                }
            }
            _ => self.release_grant(session, epoch),
        }
        self.reevaluate();
    }

    pub fn on_reclaim_result(&mut self, id: u64, ok: bool, current_epoch: u64, applied_seq: u64) {
        let Some(p) = self.pending.filter(|p| p.id == id && matches!(p.kind, PendingKind::Reclaim | PendingKind::Probe)) else { return self.reevaluate() };
        self.pending = None;
        if p.kind == PendingKind::Probe {
            self.needs_probe = false;
            if current_epoch == 0 {
                self.unsuppress();
            }
        } else if let Some(l) = self.lease.filter(|l| l.epoch == p.requested_epoch && l.generation == p.generation && l.session == p.session) {
            if ok && current_epoch == l.epoch {
                self.needs_reclaim = false;
                self.next_seq = self.next_seq.max(applied_seq);
                self.latest_sent = None;
                self.pty_sync();
            } else {
                self.invalidate();
                if current_epoch != 0 {
                    self.suppress(p.requested_epoch);
                }
            }
        }
        self.reevaluate();
    }

    pub fn on_revoked(&mut self, session: u64, revoked_epoch: u64, new_epoch: u64) {
        if self.lease.is_some_and(|l| l.session == session && l.epoch == revoked_epoch) {
            self.invalidate();
            if new_epoch != 0 {
                self.suppress(revoked_epoch);
            }
        }
        self.reevaluate();
    }

    pub fn on_vacated(&mut self) {
        self.unsuppress();
        self.reevaluate();
    }

    pub fn on_resize_ack(&mut self, session: u64, epoch: u64, seq: u64) {
        if self.lease.is_some_and(|l| l.session == session && l.epoch == epoch) && self.pty_acked.map_or(true, |a| seq > a) {
            self.pty_acked = Some(seq);
        }
        self.pty_sync();
        self.reevaluate();
    }

    pub fn on_stale_resize(&mut self, applied_epoch: u64, applied_seq: u64) {
        if self.lease.is_some_and(|l| l.epoch == applied_epoch) {
            self.next_seq = self.next_seq.max(applied_seq);
            self.latest_sent = None;
            self.pty_sync();
        }
        self.reevaluate();
    }

    pub fn on_stale_lease(&mut self, session: u64, epoch: u64, current_epoch: u64) {
        if self.lease.is_some_and(|l| l.session == session && l.epoch == epoch) {
            self.invalidate();
            if current_epoch != 0 {
                self.suppress(epoch);
            }
        }
        self.reevaluate();
    }

    pub fn on_unacked_timeout(&mut self) {
        if self.latest_sent.is_some_and(|(s, _, _)| self.pty_acked.map_or(true, |a| a < s)) {
            self.latest_sent = None;
            self.pty_sync();
        }
        self.reevaluate();
    }
}
