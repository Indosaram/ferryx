use std::collections::{BTreeMap, HashMap};

use ferryx_rd_proto::AudioFragment;

const MAX_PENDING_AUDIO_FRAMES: usize = 64;

#[derive(Debug)]
struct PendingAudioFrame {
    fragment_count: u16,
    fragments: BTreeMap<u16, Vec<u8>>,
}

/// Reassembles audio fragments and discards incomplete older frames once newer audio arrives.
#[derive(Debug, Default)]
pub struct AudioFragmentReassembler {
    pending: HashMap<u32, PendingAudioFrame>,
    newest_frame_id: Option<u32>,
    skipped_frames: u64,
}

impl AudioFragmentReassembler {
    pub fn push(&mut self, fragment: AudioFragment) -> Option<Vec<u8>> {
        let frame_id = fragment.header.frame_id;
        if self
            .newest_frame_id
            .is_some_and(|newest| frame_id != newest && frame_id.wrapping_sub(newest) >= 1 << 31)
        {
            return None;
        }

        if self.newest_frame_id.map_or(true, |newest| {
            frame_id != newest && frame_id.wrapping_sub(newest) < 1 << 31
        }) {
            self.skip_incomplete_before(frame_id);
            self.newest_frame_id = Some(frame_id);
        }

        if fragment.header.fragment_count == 1 {
            self.pending.remove(&frame_id);
            return Some(fragment.data);
        }

        let entry = self
            .pending
            .entry(frame_id)
            .or_insert_with(|| PendingAudioFrame {
                fragment_count: fragment.header.fragment_count,
                fragments: BTreeMap::new(),
            });
        if entry.fragment_count != fragment.header.fragment_count {
            self.pending.remove(&frame_id);
            self.skipped_frames += 1;
            return None;
        }
        entry
            .fragments
            .entry(fragment.header.fragment_index)
            .or_insert(fragment.data);

        if entry.fragments.len() == entry.fragment_count as usize {
            let entry = self.pending.remove(&frame_id)?;
            let total_bytes = entry.fragments.values().map(Vec::len).sum();
            let mut assembled = Vec::with_capacity(total_bytes);
            for index in 0..entry.fragment_count {
                assembled.extend_from_slice(entry.fragments.get(&index)?);
            }
            return Some(assembled);
        }

        if self.pending.len() > MAX_PENDING_AUDIO_FRAMES {
            let oldest = self.pending.keys().copied().min()?;
            self.pending.remove(&oldest);
            self.skipped_frames += 1;
        }
        None
    }

    pub fn pending_frames(&self) -> usize {
        self.pending.len()
    }

    pub fn skipped_frames(&self) -> u64 {
        self.skipped_frames
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.newest_frame_id = None;
    }

    fn skip_incomplete_before(&mut self, frame_id: u32) {
        let before = self.pending.len();
        self.pending
            .retain(|id, _| *id == frame_id || id.wrapping_sub(frame_id) < 1 << 31);
        self.skipped_frames += (before - self.pending.len()) as u64;
    }
}
