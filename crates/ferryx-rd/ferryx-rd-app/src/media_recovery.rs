//! Negotiated video repair and bounded per-frame NACK scheduling.
use super::*;
use ferryx_rd_proto::{FrameNack, FrameParity, MAX_NACK_INDICES};

const NACK_GRACE: Duration = Duration::from_millis(4);
const NACK_INTERVAL: Duration = Duration::from_millis(12);
const RECOVERY_DEADLINE: Duration = Duration::from_millis(100);
const MAX_NACKS: u8 = 3;
const MAX_PARITY_BYTES: usize = 2 * 1024 * 1024;

impl FrameAssembler {
    pub fn push_parity(
        &mut self,
        parity: FrameParity,
        now: Instant,
    ) -> Result<Option<AssembledFrame>, MediaAssemblyError> {
        self.completed_started_at = None;
        self.expire(now);
        let frame_id = parity.frame_id;
        let Some(frame) = self.frames.get_mut(&frame_id) else {
            return Ok(None);
        };
        if parity.parity_count == 0
            || parity.parity_count > frame.header.total_chunks
            || parity.parity_index >= parity.parity_count
            || parity.data.len() > ferryx_rd_proto::MAX_VIDEO_CHUNK_BYTES
            || frame
                .parity
                .values()
                .any(|old| old.parity_count != parity.parity_count)
        {
            return Err(MediaAssemblyError::InvalidChunkIndex);
        }
        let retained: usize = frame.parity.values().map(|p| p.data.len()).sum();
        if !frame.parity.contains_key(&parity.parity_index)
            && retained + parity.data.len() <= MAX_PARITY_BYTES
        {
            frame.parity.insert(parity.parity_index, parity);
        }
        self.repair(frame_id);
        self.finish_if_complete(frame_id, now)
    }

    pub(super) fn repair(&mut self, frame_id: u32) {
        let Some(frame) = self.frames.get_mut(&frame_id) else {
            return;
        };
        for parity in frame.parity.values() {
            if let Some((index, bytes)) =
                ferryx_rd_proto::recover_chunk(parity, frame.header.total_chunks, |i| {
                    frame.chunks.get(&i).map(Vec::as_slice)
                })
            {
                if frame.payload_bytes + bytes.len() <= frame.header.total_size as usize {
                    frame.payload_bytes += bytes.len();
                    frame.chunks.insert(index, bytes);
                    frame.recovered += 1;
                }
            }
        }
    }

    /// Prioritize the earliest eligible incomplete dependency, not the newest
    /// header: continuously arriving frames must not starve reference repair.
    /// Reordering gets 4 ms, with at most three requests 12 ms apart; measured
    /// RTT must fit the remaining 100 ms assembly budget.
    pub fn recovery_nack(&mut self, now: Instant, loss_ratio: f64) -> Option<FrameNack> {
        self.recovery_nack_with_rtt(now, loss_ratio, Duration::ZERO)
    }

    pub fn recovery_nack_with_rtt(
        &mut self,
        now: Instant,
        loss_ratio: f64,
        rtt: Duration,
    ) -> Option<FrameNack> {
        let newest = self.expected_frame_id?.wrapping_sub(1);
        let id = self
            .frames
            .iter()
            .filter(|(_, frame)| {
                let age = now.saturating_duration_since(frame.started);
                age >= NACK_GRACE
                    && age.saturating_add(rtt) < RECOVERY_DEADLINE
                    && frame.nack_count < MAX_NACKS
                    && frame.nack_at.map_or(true, |last| {
                        now.saturating_duration_since(last) >= NACK_INTERVAL
                    })
                    && !Self::complete(frame)
            })
            .max_by_key(|(id, _)| newest.wrapping_sub(**id))
            .map(|(id, _)| *id)?;
        let frame = self.frames.get_mut(&id)?;
        let age = now.saturating_duration_since(frame.started);
        if age.saturating_add(rtt) >= RECOVERY_DEADLINE
            || !(NACK_GRACE..RECOVERY_DEADLINE).contains(&age)
            || frame.nack_count >= MAX_NACKS
            || frame
                .nack_at
                .is_some_and(|last| now.saturating_duration_since(last) < NACK_INTERVAL)
        {
            return None;
        }
        let missing: Vec<_> = (0..frame.header.total_chunks)
            .filter(|index| !frame.chunks.contains_key(index))
            .take(MAX_NACK_INDICES)
            .collect();
        if missing.is_empty() {
            return None;
        }
        frame.nack_at = Some(now);
        frame.nack_count += 1;
        Some(FrameNack {
            frame_id: id,
            loss_permille: (loss_ratio.clamp(0.0, 1.0) * 1000.0).round() as u16,
            rtt_ms: rtt.as_micros().div_ceil(1000).min(u128::from(u16::MAX)) as u16,
            missing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(id: u32, count: u16) -> FrameHeader {
        FrameHeader {
            frame_id: id,
            width: 2,
            height: 2,
            is_key_frame: false,
            total_chunks: count,
            total_size: u32::from(count) * 3 - 2,
        }
    }

    #[test]
    fn repair_before_or_after_remaining_chunks_is_byte_exact() {
        let now = Instant::now();
        let data = [vec![1, 2, 3], vec![4, 5, 6], vec![7]];
        for missing in 0..3 {
            for parity_first in [false, true] {
                let mut frames = FrameAssembler::default();
                frames.push_header(header(1, 3), 55, now).unwrap();
                let parity = ferryx_rd_proto::build_parity(1, &data, 1).remove(0);
                if parity_first {
                    assert!(frames.push_parity(parity.clone(), now).unwrap().is_none());
                }
                let mut result = None;
                for (index, bytes) in data.iter().enumerate() {
                    if index != missing {
                        result = frames
                            .push_chunk(
                                FrameChunk {
                                    frame_id: 1,
                                    chunk_index: index as u16,
                                    data: bytes.clone(),
                                },
                                now,
                            )
                            .unwrap()
                            .or(result);
                    }
                }
                if !parity_first {
                    result = frames.push_parity(parity, now).unwrap();
                }
                let frame = result.unwrap();
                assert_eq!(frame.data, [1, 2, 3, 4, 5, 6, 7]);
                assert_eq!(frame.timestamp_ms, 55);
                // Recovered loss must remain visible to ABR rather than
                // making parity mask congestion and induce an upward probe.
                assert_eq!(frames.chunk_loss_ratio(now), 1.0 / 3.0);
            }
        }
    }

    #[test]
    fn nack_grace_retry_budget_deadline_and_latest_frame_are_exact() {
        let now = Instant::now();
        let mut frames = FrameAssembler::default();
        frames.push_header(header(u32::MAX, 3), 0, now).unwrap();
        frames
            .push_chunk(
                FrameChunk {
                    frame_id: u32::MAX,
                    chunk_index: 0,
                    data: vec![1, 2, 3],
                },
                now,
            )
            .unwrap();
        assert!(frames
            .recovery_nack(now + NACK_GRACE - Duration::from_nanos(1), 0.125)
            .is_none());
        let nack = frames.recovery_nack(now + NACK_GRACE, 0.125).unwrap();
        assert_eq!(nack.missing, [1, 2]);
        assert_eq!(nack.loss_permille, 125);
        assert!(frames.recovery_nack(now + NACK_GRACE, 0.0).is_none());
        assert!(frames
            .recovery_nack(now + NACK_GRACE + NACK_INTERVAL, 0.0)
            .is_some());
        assert!(frames
            .recovery_nack(now + NACK_GRACE + NACK_INTERVAL * 2, 0.0)
            .is_some());
        assert!(frames
            .recovery_nack(now + NACK_GRACE + NACK_INTERVAL * 3, 0.0)
            .is_none());
        frames.push_header(header(0, 3), 0, now).unwrap();
        assert_eq!(
            frames
                .recovery_nack(now + NACK_GRACE, 0.0)
                .unwrap()
                .frame_id,
            0
        );
        assert!(frames.recovery_nack(now + RECOVERY_DEADLINE, 0.0).is_none());
    }

    #[test]
    fn nack_requires_rtt_to_fit_remaining_frame_deadline() {
        let now = Instant::now();
        let mut frames = FrameAssembler::default();
        frames.push_header(header(1, 3), 0, now).unwrap();
        let at = now + Duration::from_millis(20);
        assert!(frames
            .recovery_nack_with_rtt(at, 0.1, Duration::from_millis(80))
            .is_none());
        assert!(frames
            .recovery_nack_with_rtt(at, 0.1, Duration::from_millis(81))
            .is_none());
        let nack = frames
            .recovery_nack_with_rtt(at, 0.1, Duration::from_micros(78_001))
            .unwrap();
        assert_eq!(nack.rtt_ms, 79);
    }

    #[test]
    fn continuous_headers_do_not_starve_earlier_dependencies() {
        let now = Instant::now();
        let mut frames = FrameAssembler::default();
        frames.push_header(header(u32::MAX, 3), 0, now).unwrap();
        frames
            .push_header(header(0, 3), 0, now + Duration::from_millis(16))
            .unwrap();
        let at = now + Duration::from_millis(20);
        let rtt = Duration::from_millis(10);
        assert_eq!(
            frames
                .recovery_nack_with_rtt(at, 0.1, rtt)
                .unwrap()
                .frame_id,
            u32::MAX
        );
        // While the earlier request waits for its retry interval, repair the
        // next dependency rather than blocking all recovery behind one frame.
        assert_eq!(
            frames
                .recovery_nack_with_rtt(at, 0.1, rtt)
                .unwrap()
                .frame_id,
            0
        );
        frames
            .push_header(header(1, 3), 0, now + Duration::from_millis(32))
            .unwrap();
        assert_eq!(
            frames
                .recovery_nack_with_rtt(now + Duration::from_millis(36), 0.1, rtt)
                .unwrap()
                .frame_id,
            u32::MAX
        );
        // Earlier frames cannot consume a budget they can no longer meet.
        assert_eq!(
            frames
                .recovery_nack_with_rtt(now + Duration::from_millis(90), 0.1, rtt)
                .unwrap()
                .frame_id,
            0
        );
    }

    #[test]
    fn invalid_and_orphan_parity_cannot_allocate_or_repair() {
        let now = Instant::now();
        let mut frames = FrameAssembler::default();
        let mut parity = ferryx_rd_proto::build_parity(1, &[vec![1]], 1).remove(0);
        assert!(frames.push_parity(parity.clone(), now).unwrap().is_none());
        assert!(frames.frames.is_empty());
        frames.push_header(header(1, 3), 0, now).unwrap();
        parity.parity_count = 4;
        assert!(frames.push_parity(parity, now).is_err());
        assert!(frames.frames[&1].parity.is_empty());
    }
}
