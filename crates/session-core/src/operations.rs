use std::collections::BTreeMap;

use fxsh::types::{OperationId, OperationKind};
use fxsh::Uuid;

pub const RESERVED_TTL_MS: u64 = 24 * 3600 * 1000;
pub const COMPLETED_TTL_MS: u64 = 25 * 3600 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    Reserved { at_ms: u64 },
    Completed { hash: u128, result: Vec<u8>, session: Option<[u8; 16]>, done_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Execute,
    Replay(Vec<u8>),
    Unknown,
    Expired,
    Conflict,
}

#[derive(Debug)]
pub struct OperationTable {
    host: Uuid,
    max_issued: u64,
    entries: BTreeMap<u64, (OperationKind, Stored)>,
}

pub fn request_hash(command: u16, session_id: &[u8; 16], payload: &[u8]) -> u128 {
    let mut buf = Vec::with_capacity(18 + payload.len());
    buf.extend_from_slice(&command.to_be_bytes());
    buf.extend_from_slice(session_id);
    buf.extend_from_slice(payload);
    xxhash_rust::xxh3::xxh3_128(&buf)
}

impl OperationTable {
    pub fn new(host: Uuid) -> Self {
        OperationTable { host, max_issued: 0, entries: BTreeMap::new() }
    }

    pub fn reserve(&mut self, kind: OperationKind, now_ms: u64) -> OperationId {
        self.max_issued += 1;
        self.entries.insert(self.max_issued, (kind, Stored::Reserved { at_ms: now_ms }));
        OperationId { host_instance_id: self.host, op_seq: self.max_issued }
    }

    pub fn reserved_count(&self) -> usize {
        self.entries.values().filter(|(_, s)| matches!(s, Stored::Reserved { .. })).count()
    }

    pub fn decide(&self, id: &OperationId, kind: OperationKind, hash: u128) -> Decision {
        if id.host_instance_id != self.host || id.op_seq > self.max_issued || id.op_seq == 0 {
            return Decision::Unknown;
        }
        match self.entries.get(&id.op_seq) {
            None => Decision::Expired,
            Some((k, _)) if *k != kind => Decision::Conflict,
            Some((_, Stored::Reserved { .. })) => Decision::Execute,
            Some((_, Stored::Completed { hash: h, result, .. })) if *h == hash => Decision::Replay(result.clone()),
            Some(_) => Decision::Conflict,
        }
    }

    pub fn complete(&mut self, id: &OperationId, hash: u128, result: Vec<u8>, session: Option<[u8; 16]>, now_ms: u64) {
        if let Some((_, slot)) = self.entries.get_mut(&id.op_seq) {
            *slot = Stored::Completed { hash, result, session, done_ms: now_ms };
        }
    }

    pub fn evict(&mut self, now_ms: u64, live_session: impl Fn(&[u8; 16]) -> Option<u64>) {
        self.entries.retain(|_, (kind, s)| match s {
            Stored::Reserved { at_ms } => now_ms < *at_ms + RESERVED_TTL_MS,
            Stored::Completed { session: Some(sid), done_ms, .. } if *kind == OperationKind::Spawn => match live_session(sid) {
                None => true,
                Some(ended_ms) => now_ms < ended_ms.max(*done_ms) + COMPLETED_TTL_MS,
            },
            Stored::Completed { done_ms, .. } => now_ms < *done_ms + COMPLETED_TTL_MS,
        });
    }
}
