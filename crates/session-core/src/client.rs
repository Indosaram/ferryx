use fxsh::types::{Body, Delta, SnapshotFrame, SnapshotPayload, UiEvent};
use fxsh::wire::Reader;
use fxsh::{Codec, Uuid};

use crate::replica::Replica;

pub const MAX_BODY: usize = crate::replica::S_MAX + 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientOut {
    Resync,
    Applied { revision: u64, events: Vec<UiEvent>, ui_event_gap: bool },
    DigestMismatch,
}

#[derive(Debug)]
struct Assembly {
    snapshot_id: u64,
    total: u32,
    total_len: u32,
    next_index: u32,
    bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct ReplicaClient {
    pub incarnation: Uuid,
    pub subscription_id: Option<u64>,
    pub replica: Option<Replica>,
    pub local_revision: u64,
    last_applied_snapshot: u64,
    discarded_snapshot: u64,
    resync_requested: bool,
    assembly: Option<Assembly>,
    awaiting_first: bool,
    buffered: Vec<Delta>,
    last_ui_event_id: u64,
}

impl ReplicaClient {
    pub fn new(incarnation: Uuid) -> Self {
        ReplicaClient {
            incarnation,
            subscription_id: None,
            replica: None,
            local_revision: 0,
            last_applied_snapshot: 0,
            discarded_snapshot: 0,
            resync_requested: false,
            assembly: None,
            awaiting_first: false,
            buffered: Vec::new(),
            last_ui_event_id: 0,
        }
    }

    pub fn adopt(&mut self, subscription_id: u64, snapshot_follows: bool, revision: u64) {
        self.subscription_id = Some(subscription_id);
        self.assembly = None;
        self.buffered.clear();
        self.last_applied_snapshot = 0;
        self.discarded_snapshot = 0;
        self.resync_requested = false;
        self.awaiting_first = snapshot_follows;
        if !snapshot_follows {
            self.local_revision = revision;
        }
    }

    fn assembling(&self) -> bool {
        self.awaiting_first || self.resync_requested || self.assembly.is_some()
    }

    fn discard(&mut self, snapshot_id: u64) -> ClientOut {
        self.assembly = None;
        self.discarded_snapshot = self.discarded_snapshot.max(snapshot_id);
        self.resync_requested = true;
        ClientOut::Resync
    }

    pub fn on_snapshot(&mut self, f: &SnapshotFrame) -> Option<ClientOut> {
        if Some(f.subscription_id) != self.subscription_id || f.session_incarnation != self.incarnation {
            return None;
        }
        if f.snapshot_id <= self.last_applied_snapshot.max(self.discarded_snapshot) {
            return None;
        }
        if let Some(a) = &self.assembly {
            if f.snapshot_id < a.snapshot_id {
                return None;
            }
            if f.snapshot_id > a.snapshot_id {
                self.assembly = None;
                self.buffered.clear();
            }
        }
        match &f.payload {
            SnapshotPayload::Full(body) => Some(self.install(f.snapshot_id, body.clone())),
            SnapshotPayload::Chunk { index, total, total_len, bytes } => {
                let a = self.assembly.get_or_insert(Assembly { snapshot_id: f.snapshot_id, total: *total, total_len: *total_len, next_index: 0, bytes: Vec::new() });
                let consistent = a.total == *total && a.total_len == *total_len && a.next_index == *index && *total_len as usize <= MAX_BODY
                    && a.bytes.len() + bytes.0.len() <= *total_len as usize;
                if !consistent {
                    return Some(self.discard(f.snapshot_id));
                }
                a.bytes.extend_from_slice(&bytes.0);
                a.next_index += 1;
                if a.next_index < a.total {
                    return None;
                }
                let a = self.assembly.take().unwrap();
                if a.bytes.len() != a.total_len as usize {
                    return Some(self.discard(f.snapshot_id));
                }
                match Body::dec(&mut Reader::new(&a.bytes)) {
                    Ok(body) => Some(self.install(f.snapshot_id, body)),
                    Err(_) => Some(self.discard(f.snapshot_id)),
                }
            }
        }
    }

    fn install(&mut self, snapshot_id: u64, body: Body) -> ClientOut {
        self.assembly = None;
        self.awaiting_first = false;
        self.resync_requested = false;
        self.last_applied_snapshot = snapshot_id;
        self.replica = Some(Replica::from_body(&body.state));
        self.local_revision = body.revision;
        self.last_ui_event_id = body.next_ui_event_id.saturating_sub(1);
        if fxsh::state_digest(&body.state) != body.state_digest {
            self.buffered.clear();
            self.resync_requested = true;
            return ClientOut::DigestMismatch;
        }
        let buffered = std::mem::take(&mut self.buffered);
        let mut events = Vec::new();
        let mut sorted: Vec<Delta> = buffered.into_iter().filter(|d| d.base_revision >= body.revision).collect();
        sorted.sort_by_key(|d| d.base_revision);
        for d in sorted {
            match self.apply(&d) {
                Some(ClientOut::Applied { events: e, .. }) => events.extend(e),
                Some(other) => return other,
                None => {}
            }
        }
        ClientOut::Applied { revision: self.local_revision, events, ui_event_gap: body.ui_event_gap }
    }

    pub fn on_delta(&mut self, d: &Delta) -> Option<ClientOut> {
        if Some(d.subscription_id) != self.subscription_id {
            return None;
        }
        if self.assembling() {
            self.buffered.push(d.clone());
            return None;
        }
        self.apply(d)
    }

    fn apply(&mut self, d: &Delta) -> Option<ClientOut> {
        if d.base_revision != self.local_revision {
            self.resync_requested = true;
            return Some(ClientOut::Resync);
        }
        let replica = self.replica.as_mut()?;
        let all = replica.apply_delta(d);
        self.local_revision = d.new_revision;
        let mut events = Vec::new();
        for e in all {
            if e.event_id > self.last_ui_event_id {
                self.last_ui_event_id = e.event_id;
                events.push(e);
            }
        }
        Some(ClientOut::Applied { revision: self.local_revision, events, ui_event_gap: false })
    }
}
