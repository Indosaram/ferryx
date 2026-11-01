//! Deadline-bound recent-frame cache. Each retry is sealed with a fresh nonce.
use ferryx_rd_proto::FrameNack;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_millis(100);
const RETENTION: Duration = Duration::from_millis(200);
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_FRAMES: usize = 48;
const MAX_RETRIES: u8 = 2;

struct CachedFrame {
    id: u32,
    encoded: Instant,
    bytes: usize,
    chunks: BTreeMap<u16, (Vec<u8>, u8)>,
}
#[derive(Default)]
pub(super) struct Recovery {
    pub enabled: bool,
    loss_permille: u16,
    bytes: usize,
    frames: VecDeque<CachedFrame>,
    pending: VecDeque<(u32, u16, Duration)>,
}
impl Recovery {
    pub fn redundancy(&self) -> f64 {
        (f64::from(self.loss_permille) / 1000.0 * 2.0).clamp(0.0, 0.2)
    }
    fn prune(&mut self, now: Instant) {
        while self
            .frames
            .front()
            .is_some_and(|f| now.saturating_duration_since(f.encoded) >= RETENTION)
        {
            self.bytes -= self.frames.pop_front().unwrap().bytes;
        }
    }
    pub fn begin(&mut self, frame: u32, encoded: Instant) {
        self.prune(encoded);
        if !self.enabled {
            return;
        }
        // A wrapped frame ID must never alias old payloads.
        if let Some(index) = self.frames.iter().position(|f| f.id == frame) {
            self.bytes -= self.frames.remove(index).unwrap().bytes;
        }
        while self.frames.len() >= MAX_FRAMES {
            self.bytes -= self.frames.pop_front().unwrap().bytes;
        }
        self.frames.push_back(CachedFrame {
            id: frame,
            encoded,
            bytes: 0,
            chunks: BTreeMap::new(),
        });
    }
    pub fn retain(&mut self, index: u16, payload: &[u8]) {
        if !self.enabled || payload.len() > MAX_BYTES {
            return;
        }
        while self.bytes + payload.len() > MAX_BYTES && self.frames.len() > 1 {
            self.bytes -= self.frames.pop_front().unwrap().bytes;
        }
        if self.bytes + payload.len() > MAX_BYTES {
            return;
        }
        if let Some(frame) = self.frames.back_mut() {
            if let std::collections::btree_map::Entry::Vacant(entry) = frame.chunks.entry(index) {
                entry.insert((payload.to_vec(), 0));
                frame.bytes += payload.len();
                self.bytes += payload.len();
            }
        }
    }
    /// Queue only; the sender drains this through its paced burst dispatch.
    pub fn enqueue(&mut self, nack: FrameNack, now: Instant) {
        if !self.enabled {
            return;
        }
        self.loss_permille = nack.loss_permille;
        self.prune(now);
        let Some(frame) = self.frames.iter().find(|f| f.id == nack.frame_id) else {
            return;
        };
        let rtt = Duration::from_millis(u64::from(nack.rtt_ms));
        if nack.rtt_ms == u16::MAX
            || now
                .saturating_duration_since(frame.encoded)
                .saturating_add(rtt)
                >= DEADLINE
        {
            return;
        }
        let mut seen = BTreeSet::new();
        for index in nack.missing {
            if self.pending.len() >= ferryx_rd_proto::MAX_NACK_INDICES {
                break;
            }
            let key = (nack.frame_id, index);
            if seen.insert(index)
                && !self
                    .pending
                    .iter()
                    .any(|(id, chunk, _)| (*id, *chunk) == key)
                && frame
                    .chunks
                    .get(&index)
                    .is_some_and(|(_, retries)| *retries < MAX_RETRIES)
            {
                self.pending.push_back((key.0, key.1, rtt));
            }
        }
    }
    /// Recheck the deadline at dispatch, not merely when a NACK is received.
    pub fn next(&mut self, now: Instant) -> Option<Vec<u8>> {
        self.prune(now);
        while let Some((id, index, rtt)) = self.pending.pop_front() {
            let Some(frame) = self.frames.iter_mut().find(|f| f.id == id) else {
                continue;
            };
            if now
                .saturating_duration_since(frame.encoded)
                .saturating_add(rtt)
                >= DEADLINE
            {
                continue;
            }
            if let Some((payload, retries)) = frame.chunks.get_mut(&index) {
                if *retries < MAX_RETRIES {
                    *retries += 1;
                    return Some(payload.clone());
                }
            }
        }
        None
    }
    #[cfg(test)]
    pub fn request(&mut self, nack: FrameNack, now: Instant) -> Vec<Vec<u8>> {
        self.enqueue(nack, now);
        std::iter::from_fn(|| self.next(now)).collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn nack(id: u32) -> FrameNack {
        FrameNack {
            frame_id: id,
            loss_permille: 80,
            rtt_ms: 10,
            missing: vec![0, 0, 0, 1],
        }
    }
    #[test]
    fn receiver_rtt_bounds_admission_and_each_dispatch() {
        let now = Instant::now();
        let mut ring = Recovery {
            enabled: true,
            ..Default::default()
        };
        ring.begin(1, now);
        ring.retain(0, &[1]);
        let mut feedback = nack(1);
        feedback.rtt_ms = u16::MAX;
        ring.enqueue(feedback.clone(), now);
        assert!(ring.next(now).is_none());
        feedback.rtt_ms = 80;
        ring.enqueue(feedback.clone(), now + Duration::from_millis(20));
        assert!(ring.next(now + Duration::from_millis(20)).is_none());
        feedback.rtt_ms = 79;
        ring.enqueue(feedback.clone(), now + Duration::from_millis(20));
        assert!(ring.next(now + Duration::from_millis(21)).is_none());
        ring.enqueue(feedback, now + Duration::from_millis(20));
        assert_eq!(ring.next(now + Duration::from_millis(20)), Some(vec![1]));
    }

    #[test]
    fn recent_frames_survive_new_frame_and_deduplicate_requests() {
        let now = Instant::now();
        let mut ring = Recovery {
            enabled: true,
            ..Default::default()
        };
        ring.begin(u32::MAX, now);
        ring.retain(0, &[1, 2, 3]);
        ring.begin(0, now + Duration::from_millis(16));
        ring.retain(0, &[4]);
        assert_eq!(
            ring.request(nack(u32::MAX), now + Duration::from_millis(20)),
            [vec![1, 2, 3]]
        );
        assert_eq!(
            ring.request(nack(u32::MAX), now + Duration::from_millis(25)),
            [vec![1, 2, 3]]
        );
        assert!(ring
            .request(nack(u32::MAX), now + Duration::from_millis(26))
            .is_empty());
        assert_eq!(ring.redundancy(), 0.16);
    }
    #[test]
    fn queued_retry_expires_at_exact_encode_deadline() {
        let now = Instant::now();
        let mut ring = Recovery {
            enabled: true,
            ..Default::default()
        };
        ring.begin(1, now);
        ring.retain(0, &[1]);
        ring.enqueue(nack(1), now + DEADLINE - Duration::from_nanos(1));
        assert!(ring.next(now + DEADLINE).is_none());
        ring.prune(now + RETENTION);
        assert_eq!(ring.bytes, 0);
        assert!(ring.frames.is_empty());
    }
    #[test]
    fn memory_frame_and_queue_bounds_hold() {
        let now = Instant::now();
        let mut ring = Recovery {
            enabled: true,
            ..Default::default()
        };
        ring.begin(1, now);
        ring.retain(0, &vec![0; MAX_BYTES]);
        ring.retain(1, &[1]);
        assert_eq!(ring.bytes, MAX_BYTES);
        ring.begin(2, now);
        ring.retain(0, &[2]);
        assert_eq!(ring.bytes, 1);
        for id in 3..100 {
            ring.begin(id, now);
        }
        assert_eq!(ring.frames.len(), MAX_FRAMES);
        assert!(ring.frames.iter().all(|f| f.id != 1));
    }
    #[test]
    fn disabled_peer_has_no_cache_or_redundancy_effect() {
        let now = Instant::now();
        let mut ring = Recovery::default();
        ring.begin(1, now);
        ring.retain(0, &[1]);
        assert!(ring.request(nack(1), now).is_empty());
        assert!(ring.frames.is_empty());
        assert_eq!(ring.bytes, 0);
        assert_eq!(ring.redundancy(), 0.0);
    }
}
