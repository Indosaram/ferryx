use std::collections::VecDeque;

use fxsh::messages::SubscribeAck;
use fxsh::types::Delta;
use fxsh::Uuid;

use crate::replica::{delta_encoded_len, merge};

pub const QUEUE_BYTES_LIMIT: usize = 4 * 1024 * 1024;
pub const QUEUE_MSG_LIMIT: usize = 256;
pub const SLOW_RESYNC_LIMIT: u32 = 3;
pub const SEND_DEADLINE_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetState {
    Idle,
    Waiting { request_seq: u64 },
    Sending { reservation_id: u64, snapshot_id: u64, deadline_ms: u64 },
    Draining { reservation_id: u64, snapshot_id: u64, deadline_ms: u64 },
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    SendAck(SubscribeAck),
    SendDelta(Delta),
    RequestReservation { subscription_id: u64, request_seq: u64 },
    CancelRequest { subscription_id: u64, request_seq: u64 },
    ReturnReservation { reservation_id: u64 },
    BuildSnapshot { subscription_id: u64, snapshot_id: u64, reservation_id: u64 },
    DropPending { subscription_id: u64, snapshot_id: u64 },
    ArmDeadline { subscription_id: u64, reservation_id: u64, snapshot_id: u64, at_ms: u64 },
    CancelDeadline { subscription_id: u64 },
    CloseSlowConsumer { subscription_id: u64 },
}

#[derive(Debug, Clone)]
pub struct Subscription {
    pub id: u64,
    pub subscriber_id: Uuid,
    pub attach_seq: u64,
    pub base: u64,
    pub resync_pending: bool,
    pub resync_count: u32,
    pub request_seq: u64,
    pub next_snapshot_id: u64,
    pub budget: BudgetState,
    pub queue: VecDeque<Delta>,
    pub queue_bytes: usize,
    pub unreturned: Vec<u64>,
}

impl Subscription {
    pub fn new(id: u64, subscriber_id: Uuid, attach_seq: u64) -> Self {
        Subscription {
            id,
            subscriber_id,
            attach_seq,
            base: 0,
            resync_pending: false,
            resync_count: 0,
            request_seq: 0,
            next_snapshot_id: 1,
            budget: BudgetState::Idle,
            queue: VecDeque::new(),
            queue_bytes: 0,
            unreturned: Vec::new(),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.budget == BudgetState::Closed
    }

    pub fn start_with_ack(&mut self, incarnation: Uuid, revision: u64, known: Option<(u64, Uuid)>, fx: &mut Vec<Effect>) {
        if known == Some((revision, incarnation)) {
            fx.push(Effect::SendAck(SubscribeAck { attach_seq: self.attach_seq, subscription_id: self.id, session_incarnation: incarnation, revision, snapshot_follows: false }));
            self.base = revision;
        } else {
            fx.push(Effect::SendAck(SubscribeAck { attach_seq: self.attach_seq, subscription_id: self.id, session_incarnation: incarnation, revision: 0, snapshot_follows: true }));
            self.resync(fx);
        }
    }

    pub fn resync(&mut self, fx: &mut Vec<Effect>) {
        if self.is_closed() {
            return;
        }
        self.queue.clear();
        self.queue_bytes = 0;
        self.resync_pending = true;
        match self.budget {
            BudgetState::Sending { reservation_id, snapshot_id, deadline_ms } => {
                fx.push(Effect::DropPending { subscription_id: self.id, snapshot_id });
                self.budget = BudgetState::Draining { reservation_id, snapshot_id, deadline_ms };
            }
            BudgetState::Idle => self.request(fx),
            _ => {}
        }
    }

    fn request(&mut self, fx: &mut Vec<Effect>) {
        self.request_seq += 1;
        self.budget = BudgetState::Waiting { request_seq: self.request_seq };
        fx.push(Effect::RequestReservation { subscription_id: self.id, request_seq: self.request_seq });
    }

    pub fn on_budget_available(&mut self, request_seq: u64, reservation_id: u64, revision: u64, now_ms: u64, fx: &mut Vec<Effect>) {
        match self.budget {
            BudgetState::Waiting { request_seq: q } if q == request_seq => {
                let snapshot_id = self.next_snapshot_id;
                self.next_snapshot_id += 1;
                let deadline_ms = now_ms + SEND_DEADLINE_MS;
                self.base = revision;
                self.resync_pending = false;
                self.budget = BudgetState::Sending { reservation_id, snapshot_id, deadline_ms };
                fx.push(Effect::BuildSnapshot { subscription_id: self.id, snapshot_id, reservation_id });
                fx.push(Effect::ArmDeadline { subscription_id: self.id, reservation_id, snapshot_id, at_ms: deadline_ms });
            }
            _ => fx.push(Effect::ReturnReservation { reservation_id }),
        }
    }

    fn matches(&self, reservation_id: u64, snapshot_id: u64) -> bool {
        matches!(self.budget,
            BudgetState::Sending { reservation_id: r, snapshot_id: s, .. } | BudgetState::Draining { reservation_id: r, snapshot_id: s, .. }
            if r == reservation_id && s == snapshot_id)
    }

    pub fn on_frame_written(&mut self, reservation_id: u64, snapshot_id: u64, last: bool, fx: &mut Vec<Effect>) {
        if !last || !self.matches(reservation_id, snapshot_id) {
            return;
        }
        if let BudgetState::Sending { reservation_id, snapshot_id, deadline_ms } = self.budget {
            fx.push(Effect::CancelDeadline { subscription_id: self.id });
            self.budget = BudgetState::Draining { reservation_id, snapshot_id, deadline_ms };
            self.resync_count = 0;
        }
    }

    pub fn on_buffer_released(&mut self, reservation_id: u64, snapshot_id: u64, fx: &mut Vec<Effect>) {
        if let Some(pos) = self.unreturned.iter().position(|r| *r == reservation_id) {
            self.unreturned.remove(pos);
            fx.push(Effect::ReturnReservation { reservation_id });
            return;
        }
        if !self.matches(reservation_id, snapshot_id) {
            return;
        }
        fx.push(Effect::ReturnReservation { reservation_id });
        match self.budget {
            BudgetState::Draining { .. } => {
                if self.resync_pending {
                    self.request(fx);
                } else {
                    self.budget = BudgetState::Idle;
                }
            }
            BudgetState::Sending { .. } => {
                self.budget = BudgetState::Idle;
                self.close(fx);
            }
            _ => {}
        }
    }

    pub fn on_deadline(&mut self, reservation_id: u64, snapshot_id: u64, fx: &mut Vec<Effect>) {
        if self.matches(reservation_id, snapshot_id) {
            fx.push(Effect::CloseSlowConsumer { subscription_id: self.id });
            self.close(fx);
        }
    }

    pub fn close(&mut self, fx: &mut Vec<Effect>) {
        match self.budget {
            BudgetState::Sending { reservation_id, snapshot_id, .. } | BudgetState::Draining { reservation_id, snapshot_id, .. } => {
                fx.push(Effect::DropPending { subscription_id: self.id, snapshot_id });
                self.unreturned.push(reservation_id);
            }
            BudgetState::Waiting { request_seq } => fx.push(Effect::CancelRequest { subscription_id: self.id, request_seq }),
            BudgetState::Idle | BudgetState::Closed => {}
        }
        self.queue.clear();
        self.queue_bytes = 0;
        self.budget = BudgetState::Closed;
    }

    pub fn push_delta(&mut self, mut d: Delta, fx: &mut Vec<Effect>) {
        if self.is_closed() || self.resync_pending || !matches!(self.budget, BudgetState::Idle | BudgetState::Draining { .. } | BudgetState::Sending { .. }) {
            return;
        }
        d.subscription_id = self.id;
        if let Some(prev) = self.queue.back() {
            debug_assert_eq!(prev.new_revision, d.base_revision);
        } else {
            debug_assert_eq!(self.base, d.base_revision);
        }
        self.queue_bytes += delta_encoded_len(&d);
        self.queue.push_back(d);
        if self.queue_bytes > QUEUE_BYTES_LIMIT || self.queue.len() > QUEUE_MSG_LIMIT {
            self.resync_count += 1;
            if self.resync_count >= SLOW_RESYNC_LIMIT {
                fx.push(Effect::CloseSlowConsumer { subscription_id: self.id });
                self.close(fx);
            } else {
                self.resync(fx);
            }
        }
    }

    pub fn drain_to_wire(&mut self, fx: &mut Vec<Effect>) {
        if self.resync_pending || self.is_closed() {
            return;
        }
        if let Some(merged) = merge(self.queue.make_contiguous()) {
            self.base = merged.new_revision;
            fx.push(Effect::SendDelta(merged));
        }
        self.queue.clear();
        self.queue_bytes = 0;
    }
}
