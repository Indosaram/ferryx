use std::collections::{BTreeMap, VecDeque};

pub const RETAINED_BYTES: usize = 4 * 1024 * 1024;
pub const INPUT_QUEUE_LIMIT: usize = 1024 * 1024;
pub const VT_REPLY_LIMIT: usize = 64 * 1024;
pub const MAX_CHUNK: usize = 64 * 1024;
pub const RECENT_EPOCHS: usize = 64;
pub const MAX_UNWRITTEN_EPOCHS: usize = 64;

pub type Holder = [u8; 16];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochLedger {
    pub holder: Holder,
    pub accepted: u64,
    pub committed: u64,
    pub pending_dropped: u64,
    pub fenced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Input { epoch: u64, bytes: Vec<u8>, written: usize },
    Vt { bytes: Vec<u8>, written: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    StaleLease { current_epoch: u64, epoch_final_accepted: u64 },
    TooLarge,
    CorruptFrame,
    OffsetOverflow,
    Gap { accepted: u64 },
    Diverged { epoch: u64, accepted: u64, committed: u64 },
    Unverifiable { epoch: u64, accepted: u64, committed: u64, retained_start: u64 },
    Backpressure { accepted: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ack {
    pub holder: Holder,
    pub epoch: u64,
    pub accepted: u64,
    pub committed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revoke {
    pub holder: Holder,
    pub revoked_epoch: u64,
    pub new_epoch: u64,
}

#[derive(Debug, Default)]
pub struct InputLedger {
    current: u64,
    holder: Option<Holder>,
    epochs: BTreeMap<u64, EpochLedger>,
    retained: VecDeque<u8>,
    retained_start: u64,
    queue: VecDeque<Item>,
    input_bytes: usize,
    vt_bytes: usize,
    pub vt_replies_dropped: u64,
    revokees: Vec<Holder>,
    dead: bool,
    last_request: BTreeMap<Holder, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantError {
    TooManyUnwrittenEpochs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Granted {
    pub epoch: u64,
    pub prior: Option<(u64, u64, u64)>,
    pub revoked: Option<Revoke>,
}

impl InputLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_epoch(&self) -> u64 {
        self.current
    }

    pub fn holder(&self) -> Option<Holder> {
        self.holder
    }

    pub fn epoch(&self, e: u64) -> Option<&EpochLedger> {
        self.epochs.get(&e)
    }

    pub fn queued_input_bytes(&self) -> usize {
        self.input_bytes
    }

    fn unwritten_epochs(&self) -> usize {
        let mut set: Vec<u64> = self.queue.iter().filter_map(|i| if let Item::Input { epoch, .. } = i { Some(*epoch) } else { None }).collect();
        set.dedup();
        set.len()
    }

    fn fence_current(&mut self) {
        if let Some(l) = self.epochs.get_mut(&self.current) {
            l.fenced = true;
        }
        self.holder = None;
    }

    pub fn fresh(&mut self, who: Holder, request_id: u64) -> bool {
        let last = self.last_request.entry(who).or_insert(0);
        if request_id <= *last {
            return false;
        }
        *last = request_id;
        true
    }

    pub fn acquire(&mut self, who: Holder) -> Result<Granted, GrantError> {
        if self.unwritten_epochs() >= MAX_UNWRITTEN_EPOCHS {
            return Err(GrantError::TooManyUnwrittenEpochs);
        }
        let prev_epoch = self.current;
        let prev_holder = self.holder;
        let prior = self.epochs.get(&prev_epoch).map(|l| (prev_epoch, l.accepted, l.committed));
        self.fence_current();
        self.current += 1;
        self.epochs.insert(self.current, EpochLedger { holder: who, accepted: 0, committed: 0, pending_dropped: 0, fenced: false });
        self.holder = Some(who);
        self.retained.clear();
        self.retained_start = 0;
        self.revokees.retain(|h| *h != who);
        let revoked = match prev_holder {
            Some(h) if h != who => {
                self.revokees.push(h);
                Some(Revoke { holder: h, revoked_epoch: prev_epoch, new_epoch: self.current })
            }
            _ => None,
        };
        self.prune();
        Ok(Granted { epoch: self.current, prior, revoked })
    }

    pub fn release(&mut self, who: Holder, epoch: u64) -> (bool, Vec<Holder>) {
        let epoch = if epoch == 0 { self.current } else { epoch };
        if epoch == self.current && self.holder == Some(who) && self.epochs.get(&epoch).is_some_and(|l| !l.fenced) {
            self.fence_current();
            (true, std::mem::take(&mut self.revokees))
        } else {
            (false, Vec::new())
        }
    }

    pub fn reclaim(&mut self, who: Holder, epoch: u64) -> Result<Result<(u64, u64), (u64, u64, u64, u64)>, ()> {
        let Some(l) = self.epochs.get(&epoch) else { return Err(()) };
        if epoch == self.current && self.holder == Some(who) && !l.fenced {
            return Ok(Ok((l.accepted, l.committed)));
        }
        let current = if self.holder.is_some() { self.current } else { 0 };
        if self.holder.is_some_and(|h| h != who) && !self.revokees.contains(&who) {
            self.revokees.push(who);
        }
        Ok(Err((current, l.accepted, l.committed, l.pending_dropped)))
    }

    pub fn epoch_state(&self, epoch: u64) -> Option<(u64, u64, u64, bool)> {
        self.epochs.get(&epoch).map(|l| (l.accepted, l.committed, l.pending_dropped, l.fenced))
    }

    pub fn write(&mut self, who: Holder, epoch: u64, start: u64, bytes: &[u8], crc: u32) -> Result<Ack, (WriteError, Option<(Revoke, Vec<Holder>)>)> {
        let current_ok = epoch == self.current && self.holder == Some(who) && self.epochs.get(&epoch).is_some_and(|l| !l.fenced);
        if !current_ok {
            let fa = self.epochs.get(&epoch).map_or(0, |l| l.accepted);
            let cur = if self.holder.is_some() { self.current } else { 0 };
            return Err((WriteError::StaleLease { current_epoch: cur, epoch_final_accepted: fa }, None));
        }
        if bytes.len() > MAX_CHUNK {
            return Err((WriteError::TooLarge, None));
        }
        if fxsh::input_crc32c(bytes) != crc {
            return Err((WriteError::CorruptFrame, None));
        }
        let Some(end) = start.checked_add(bytes.len() as u64) else { return Err((WriteError::OffsetOverflow, None)) };
        let l = &self.epochs[&epoch];
        let (accepted, committed) = (l.accepted, l.committed);
        if start > accepted {
            return Err((WriteError::Gap { accepted }, None));
        }
        let overlap_end = end.min(accepted);
        if start < overlap_end {
            let err = if start < self.retained_start {
                Some(WriteError::Unverifiable { epoch, accepted, committed, retained_start: self.retained_start })
            } else {
                let off = (start - self.retained_start) as usize;
                let n = (overlap_end - start) as usize;
                let same = self.retained.range(off..off + n).copied().eq(bytes[..n].iter().copied());
                (!same).then_some(WriteError::Diverged { epoch, accepted, committed })
            };
            if let Some(e) = err {
                self.fence_current();
                let revokees = std::mem::take(&mut self.revokees);
                return Err((e, Some((Revoke { holder: who, revoked_epoch: epoch, new_epoch: 0 }, revokees))));
            }
        }
        if end > accepted {
            let suffix = &bytes[(accepted - start) as usize..];
            if self.input_bytes + suffix.len() > INPUT_QUEUE_LIMIT {
                return Err((WriteError::Backpressure { accepted }, None));
            }
            self.queue.push_back(Item::Input { epoch, bytes: suffix.to_vec(), written: 0 });
            self.input_bytes += suffix.len();
            self.retained.extend(suffix.iter().copied());
            while self.retained.len() > RETAINED_BYTES {
                self.retained.pop_front();
                self.retained_start += 1;
            }
            self.epochs.get_mut(&epoch).unwrap().accepted = end;
        }
        let l = &self.epochs[&epoch];
        Ok(Ack { holder: who, epoch, accepted: l.accepted, committed: l.committed })
    }

    pub fn push_vt_reply(&mut self, bytes: Vec<u8>) -> bool {
        if self.dead || self.vt_bytes + bytes.len() > VT_REPLY_LIMIT {
            self.vt_replies_dropped += 1;
            return false;
        }
        self.vt_bytes += bytes.len();
        self.queue.push_back(Item::Vt { bytes, written: 0 });
        true
    }

    pub fn head(&self) -> Option<&[u8]> {
        self.queue.front().map(|i| match i {
            Item::Input { bytes, written, .. } | Item::Vt { bytes, written } => &bytes[*written..],
        })
    }

    pub fn on_written(&mut self, n: usize) -> Option<Ack> {
        let front = self.queue.front_mut()?;
        let mut ack = None;
        let done = match front {
            Item::Input { epoch, bytes, written } => {
                let n = n.min(bytes.len() - *written);
                *written += n;
                self.input_bytes -= n;
                let l = self.epochs.get_mut(epoch).expect("queued epoch retained");
                l.committed += n as u64;
                ack = Some(Ack { holder: l.holder, epoch: *epoch, accepted: l.accepted, committed: l.committed });
                *written == bytes.len()
            }
            Item::Vt { bytes, written } => {
                let n = n.min(bytes.len() - *written);
                *written += n;
                self.vt_bytes -= n;
                *written == bytes.len()
            }
        };
        if done {
            self.queue.pop_front();
            self.prune();
        }
        ack
    }

    pub fn on_dead(&mut self) {
        self.dead = true;
        for item in self.queue.drain(..) {
            if let Item::Input { epoch, bytes, written } = item {
                if let Some(l) = self.epochs.get_mut(&epoch) {
                    l.pending_dropped += (bytes.len() - written) as u64;
                }
            }
        }
        self.input_bytes = 0;
        self.vt_bytes = 0;
    }

    pub fn pending_dropped_by_epoch(&self) -> Vec<(u64, u64)> {
        self.epochs.iter().filter(|(_, l)| l.pending_dropped > 0).map(|(e, l)| (*e, l.pending_dropped)).collect()
    }

    fn prune(&mut self) {
        let unwritten: Vec<u64> = self.queue.iter().filter_map(|i| if let Item::Input { epoch, .. } = i { Some(*epoch) } else { None }).collect();
        let recent_floor = self.current.saturating_sub(RECENT_EPOCHS as u64 - 1);
        let current = self.current;
        self.epochs.retain(|e, _| *e == current || *e >= recent_floor || unwritten.contains(e));
    }
}
