use std::collections::BTreeMap;

use crate::input::Holder;
use crate::replica::normalize_size;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Applied {
    pub epoch: u64,
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResizeError {
    StaleLease { current_epoch: u64 },
    StaleResize(Applied),
}

#[derive(Debug, Default)]
pub struct ResizeLease {
    epoch: u64,
    holder: Option<Holder>,
    pub applied: Applied,
    revokees: Vec<Holder>,
    last_request: BTreeMap<Holder, u64>,
}

impl ResizeLease {
    pub fn new(cols: u16, rows: u16) -> Self {
        let (cols, rows) = normalize_size(cols, rows);
        ResizeLease { applied: Applied { epoch: 0, seq: 0, cols, rows }, ..Default::default() }
    }

    pub fn current(&self) -> (u64, Option<Holder>) {
        (self.epoch, self.holder)
    }

    pub fn fresh(&mut self, who: Holder, request_id: u64) -> bool {
        let last = self.last_request.entry(who).or_insert(0);
        if request_id <= *last {
            return false;
        }
        *last = request_id;
        true
    }

    pub fn acquire(&mut self, who: Holder) -> (u64, Applied, Option<(Holder, u64)>) {
        let prev = self.holder;
        let prev_epoch = self.epoch;
        self.epoch += 1;
        self.holder = Some(who);
        self.revokees.retain(|h| *h != who);
        let revoked = match prev {
            Some(h) if h != who => {
                self.revokees.push(h);
                Some((h, prev_epoch))
            }
            _ => None,
        };
        (self.epoch, self.applied, revoked)
    }

    pub fn release(&mut self, who: Holder, epoch: u64) -> (bool, Vec<Holder>) {
        if (epoch == 0 || epoch == self.epoch) && self.holder == Some(who) {
            self.holder = None;
            (true, std::mem::take(&mut self.revokees))
        } else {
            (false, Vec::new())
        }
    }

    pub fn reclaim(&mut self, who: Holder, epoch: u64) -> (bool, u64, Applied) {
        if epoch == self.epoch && self.holder == Some(who) {
            return (true, epoch, self.applied);
        }
        if self.holder.is_some_and(|h| h != who) && !self.revokees.contains(&who) {
            self.revokees.push(who);
        }
        (false, if self.holder.is_some() { self.epoch } else { 0 }, self.applied)
    }

    pub fn resize(&mut self, who: Holder, epoch: u64, seq: u64, cols: u16, rows: u16) -> Result<Applied, ResizeError> {
        if epoch != self.epoch || self.holder != Some(who) {
            return Err(ResizeError::StaleLease { current_epoch: if self.holder.is_some() { self.epoch } else { 0 } });
        }
        if (epoch, seq) <= (self.applied.epoch, self.applied.seq) {
            return Err(ResizeError::StaleResize(self.applied));
        }
        let (cols, rows) = normalize_size(cols, rows);
        self.applied = Applied { epoch, seq, cols, rows };
        Ok(self.applied)
    }
}
