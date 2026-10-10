use std::collections::{BTreeMap, VecDeque};

pub type SessionKey = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestKey {
    pub session: SessionKey,
    pub subscription_id: u64,
    pub request_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    pub key: RequestKey,
    pub reservation_id: u64,
}

#[derive(Debug, Clone)]
pub struct Allocator {
    budget: u64,
    reservation_size: u64,
    queue: VecDeque<RequestKey>,
    issued: BTreeMap<u64, RequestKey>,
    next_reservation: u64,
}

impl Allocator {
    pub fn new(budget: u64, reservation_size: u64) -> Self {
        assert!(reservation_size > 0 && reservation_size <= budget);
        Allocator { budget, reservation_size, queue: VecDeque::new(), issued: BTreeMap::new(), next_reservation: 1 }
    }

    pub fn issued_bytes(&self) -> u64 {
        self.issued.len() as u64 * self.reservation_size
    }

    pub fn issued_count(&self) -> usize {
        self.issued.len()
    }

    pub fn queued(&self) -> impl Iterator<Item = &RequestKey> {
        self.queue.iter()
    }

    pub fn request(&mut self, key: RequestKey) -> Vec<Grant> {
        let dup = self.queue.iter().any(|k| k.session == key.session && k.subscription_id == key.subscription_id);
        if !dup {
            self.queue.push_back(key);
        }
        self.pump()
    }

    pub fn cancel(&mut self, key: RequestKey) -> Vec<Grant> {
        self.queue.retain(|k| *k != key);
        self.pump()
    }

    pub fn give_back(&mut self, reservation_id: u64) -> Vec<Grant> {
        self.issued.remove(&reservation_id);
        self.pump()
    }

    fn pump(&mut self) -> Vec<Grant> {
        let mut out = Vec::new();
        while let Some(head) = self.queue.front().copied() {
            if self.issued_bytes() + self.reservation_size > self.budget {
                break;
            }
            self.queue.pop_front();
            let id = self.next_reservation;
            self.next_reservation += 1;
            self.issued.insert(id, head);
            out.push(Grant { key: head, reservation_id: id });
        }
        out
    }
}
