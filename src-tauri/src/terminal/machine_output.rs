//! Machine-only output admission. No producer waits for socket progress.
use super::{AttachmentSnapshot, HistoryRange, OutputChunk, SessionAttachment, TerminalOutputHub};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};

pub const MACHINE_OUTPUT_BYTES: usize = 1024 * 1024;
/// Conservative allowance for OSC metadata, reset, and WebSocket header.
pub const MACHINE_FRAME_OVERHEAD: usize = 512;
/// Reserved for the initial attached JSON and other serialized socket controls.
pub const MACHINE_CONTROL_BYTES: usize = 16 * 1024;
const MAX_ENTRIES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MachineOutputError {
    #[error("machine output budget exceeded")]
    Overflow,
    #[error("machine output closed")]
    Closed,
}

/// Keep this value alive until its encoded frame has been flushed or discarded.
/// Cloning payloads into another pending queue and dropping this guard is forbidden.
#[derive(Debug)]
pub struct ChargedOutput<T> {
    pub value: T,
    _charge: OwnedSemaphorePermit,
}

#[derive(Debug)]
pub struct MachineAttachment {
    pub snapshot: ChargedOutput<AttachmentSnapshot>,
    pub receiver: MachineReceiver,
    pub history_ranges: Vec<HistoryRange>,
}

#[derive(Debug)]
pub struct MachineReceiver {
    receiver: mpsc::Receiver<ChargedOutput<OutputChunk>>,
    status: watch::Receiver<Option<MachineOutputError>>,
    budget: Arc<Semaphore>,
    _controls: OwnedSemaphorePermit,
}

impl MachineReceiver {
    /// Clone before borrowing `recv`, and select this signal against EVERY write.
    /// `wait_for(|state| state.is_some())` also observes an already-fired overflow.
    pub fn termination(&self) -> watch::Receiver<Option<MachineOutputError>> {
        self.status.clone()
    }

    pub fn pending_bytes(&self) -> usize {
        MACHINE_OUTPUT_BYTES - self.budget.available_permits()
    }

    pub fn try_charge(&self, bytes: usize) -> Result<OwnedSemaphorePermit, MachineOutputError> {
        charge(&self.budget, bytes)
    }

    pub async fn recv(&mut self) -> Result<ChargedOutput<OutputChunk>, MachineOutputError> {
        if let Some(error) = *self.status.borrow() {
            self.receiver.close();
            while self.receiver.try_recv().is_ok() {}
            return Err(error);
        }
        tokio::select! {
            biased;
            changed = self.status.changed() => {
                let error = match changed {
                    Ok(()) => (*self.status.borrow()).unwrap_or(MachineOutputError::Closed),
                    Err(_) => MachineOutputError::Closed,
                };
                self.receiver.close();
                while self.receiver.try_recv().is_ok() {}
                Err(error)
            }
            output = self.receiver.recv() => output.ok_or(MachineOutputError::Closed),
        }
    }
}

pub(super) struct MachineSender {
    sender: mpsc::Sender<ChargedOutput<OutputChunk>>,
    status: watch::Sender<Option<MachineOutputError>>,
    budget: Arc<Semaphore>,
}

fn charge(
    budget: &Arc<Semaphore>,
    bytes: usize,
) -> Result<OwnedSemaphorePermit, MachineOutputError> {
    let permits = u32::try_from(bytes).map_err(|_| MachineOutputError::Overflow)?;
    Arc::clone(budget)
        .try_acquire_many_owned(permits)
        .map_err(|_| MachineOutputError::Overflow)
}

impl MachineSender {
    pub(super) fn publish(&self, chunk: &OutputChunk) -> bool {
        if self.sender.is_closed() {
            return false;
        }
        let result = chunk
            .bytes
            .len()
            .checked_add(MACHINE_FRAME_OVERHEAD)
            .ok_or(MachineOutputError::Overflow)
            .and_then(|size| charge(&self.budget, size));
        match result {
            Ok(permit) => {
                let output = ChargedOutput {
                    value: chunk.clone(),
                    _charge: permit,
                };
                if self.sender.try_send(output).is_ok() {
                    return true;
                }
            }
            Err(MachineOutputError::Overflow) => {}
            Err(MachineOutputError::Closed) => return false,
        }
        self.status.send_replace(Some(MachineOutputError::Overflow));
        false
    }
}

impl Drop for MachineSender {
    fn drop(&mut self) {
        self.status.send_if_modified(|status| {
            if status.is_none() {
                *status = Some(MachineOutputError::Closed);
                true
            } else {
                false
            }
        });
    }
}

impl TerminalOutputHub {
    /// Atomic machine subscription plus replay. Missing session is `None`;
    /// over-budget replay is `Some(Err(Overflow))`, never truncated silently.
    /// Machine wire uses flat history; segmented Local/SSH snapshots are unchanged.
    pub fn subscribe_machine(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Option<Result<MachineAttachment, MachineOutputError>> {
        let session = self.sessions.read().get(session_id).cloned()?;
        let mut hub = session.write();
        let budget = Arc::new(Semaphore::new(MACHINE_OUTPUT_BYTES));
        let (sender, receiver) = mpsc::channel(MAX_ENTRIES);
        let (status_tx, status) = watch::channel(None);
        // Install before snapshot, under the same publisher lock.
        hub.machine_senders
            .retain(|sender| !sender.sender.is_closed());
        hub.machine_senders.push(MachineSender {
            sender,
            status: status_tx,
            budget: Arc::clone(&budget),
        });
        let build = || {
            let controls = charge(&budget, MACHINE_CONTROL_BYTES)?;
            let (history, history_ranges, history_start_sequence, history_end_sequence, gap) = hub
                .buffer
                .snapshot_after_ranges(after_sequence, &hub.resize_ledger);
            let gap = gap.or_else(|| {
                hub.replay_gap.clone().filter(|gap| {
                    after_sequence.is_none_or(|after| after < gap.available_from_sequence - 1)
                })
            });
            let mut replay = SessionAttachment {
                snapshot: AttachmentSnapshot {
                    session_id: session_id.to_owned(),
                    history_start_sequence,
                    history_end_sequence,
                    history,
                    history_segments: Vec::new(),
                    gap,
                    history_incomplete: hub.history_incomplete(),
                },
                // The shared replay boundary accepts a SessionAttachment; live machine
                // output still uses only the charged receiver installed above.
                receiver: hub.sender.subscribe(),
            };
            let history_ranges = super::super::remote::clean_replay(
                &mut replay,
                history_ranges,
                after_sequence,
            );
            // Charge the actual outgoing history, including retained prelude bytes,
            // after cleanup has remapped the ranges. Never estimate from raw ring chunks.
            let range_count = hub.resize_ledger.len().saturating_add(1);
            let range_capacity = range_count.next_power_of_two().max(4);
            let max_range_bytes = range_capacity.saturating_mul(std::mem::size_of::<HistoryRange>());
            let permit = charge(
                &budget,
                replay.snapshot.history.len()
                    .saturating_add(8 + MACHINE_FRAME_OVERHEAD + max_range_bytes),
            )?;
            Ok(MachineAttachment {
                snapshot: ChargedOutput {
                    value: replay.snapshot,
                    _charge: permit,
                },
                receiver: MachineReceiver {
                    receiver,
                    status,
                    budget: Arc::clone(&budget),
                    _controls: controls,
                },
                history_ranges,
            })
        };
        let result = build();
        if result.is_err() {
            hub.machine_senders.pop();
        } else {
            hub.subscribed = true;
            if hub.ended {
                drop(hub);
                self.remove_ended_session(session_id);
            }
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_full_replay_strips_device_queries_and_remaps_ranges() {
        let hub = TerminalOutputHub::new(4096);
        hub.register_session("full");
        hub.publish("full", b"first\x1b[5n\x1b]11;?\x07".to_vec());
        hub.record_resize("full", 120, 40);
        hub.publish("full", b"second\x1b[c\x1b[6n\x1b[?6n\x1b[1;1R".to_vec());
        let attachment = hub.subscribe_machine("full", None)
            .expect("session exists").expect("attach admitted");
        assert_eq!(attachment.snapshot.value.history, b"firstsecond\x1b[6n\x1b[?6n\x1b[1;1R");
        assert_eq!(attachment.history_ranges.len(), 2);
        let history = &attachment.snapshot.value.history;
        let first = &attachment.history_ranges[0];
        let second = &attachment.history_ranges[1];
        assert_eq!(&history[first.start..first.end], b"first");
        assert_eq!(&history[second.start..second.end], b"second\x1b[6n\x1b[?6n\x1b[1;1R");
        assert_eq!((second.cols, second.rows), (Some(120), Some(40)));
    }

    #[test]
    fn machine_delta_replay_preserves_utf8_boundary_even_with_gap() {
        let hub = TerminalOutputHub::new(4096);
        hub.register_session("delta");
        let lead = hub.publish("delta", vec![0xe2]).expect("publish lead").sequence;
        let tail = b"\x82\xac\x1b[5n";
        hub.publish("delta", tail.to_vec());
        // A cursor older than the retained head still classifies as a delta.
        for after in [lead, 0] {
            if after == 0 {
                let session = hub.sessions.read().get("delta").cloned().expect("session exists");
                session.write().replay_gap = Some(super::super::ReplayGap {
                    requested_after_sequence: 0,
                    available_from_sequence: 2,
                });
            }
            let attachment = hub.subscribe_machine("delta", Some(after))
                .expect("session exists").expect("attach admitted");
            if after == lead {
                assert_eq!(attachment.snapshot.value.history, tail);
                let mut joined = vec![0xe2];
                joined.extend_from_slice(&attachment.snapshot.value.history[..2]);
                assert_eq!(String::from_utf8(joined).expect("valid UTF-8"), "\u{20ac}");
            } else {
                assert!(attachment.snapshot.value.gap.is_some());
                assert_eq!(attachment.snapshot.value.history, b"\xe2\x82\xac\x1b[5n");
            }
        }
    }

    #[test]
    fn machine_replay_admission_uses_post_clean_size_at_the_byte_cap() {
        let overhead = MACHINE_CONTROL_BYTES + 8 + MACHINE_FRAME_OVERHEAD
            + 4 * std::mem::size_of::<HistoryRange>();
        let payload_limit = MACHINE_OUTPUT_BYTES - overhead;
        let hub = TerminalOutputHub::new(2 * MACHINE_OUTPUT_BYTES);
        hub.register_session("fits");
        let mut raw = vec![b'x'; payload_limit];
        raw.extend_from_slice(&b"\x1b[5n".repeat(8192));
        assert!(raw.len() > MACHINE_OUTPUT_BYTES);
        hub.publish("fits", raw);
        let attachment = hub.subscribe_machine("fits", None)
            .expect("session exists").expect("cleaned replay fits exactly");
        assert_eq!(attachment.snapshot.value.history, vec![b'x'; payload_limit]);
        assert_eq!(attachment.receiver.pending_bytes(), MACHINE_OUTPUT_BYTES);
        assert_eq!(attachment.receiver.try_charge(1).expect_err("cap exhausted"), MachineOutputError::Overflow);
        drop(attachment.snapshot);
        assert_eq!(attachment.receiver.pending_bytes(), MACHINE_CONTROL_BYTES);

        hub.register_session("oversize");
        let mut raw = vec![b'x'; payload_limit + 1];
        raw.extend_from_slice(b"\x1b[5n");
        hub.publish("oversize", raw);
        assert_eq!(hub.subscribe_machine("oversize", None)
            .expect("session exists").expect_err("cleaned replay exceeds cap"), MachineOutputError::Overflow);
        let session = hub.sessions.read().get("oversize").cloned().expect("session exists");
        assert!(session.read().machine_senders.is_empty(), "refused subscription is removed");
    }

    #[test]
    fn machine_replay_cap_cannot_be_bypassed_by_a_smaller_recovery_ring() {
        let hub = TerminalOutputHub::new_with_retention(
            1024,
            super::super::retained_history::RetentionBudget::from_text_capacity(2 * MACHINE_OUTPUT_BYTES),
        );
        hub.register_session("retained");
        hub.publish("retained", vec![b'x'; MACHINE_OUTPUT_BYTES]);
        hub.publish("retained", b"tail\x1b[5n".to_vec());
        assert_eq!(hub.subscribe_machine("retained", None)
            .expect("session exists").expect_err("retained replay exceeds cap after cleaning"), MachineOutputError::Overflow);
    }
}
