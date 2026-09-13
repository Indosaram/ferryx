//! Machine-only output admission. No producer waits for socket progress.
use super::{AttachmentSnapshot, OutputChunk, TerminalOutputHub};
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

fn charge(budget: &Arc<Semaphore>, bytes: usize) -> Result<OwnedSemaphorePermit, MachineOutputError> {
    let permits = u32::try_from(bytes).map_err(|_| MachineOutputError::Overflow)?;
    Arc::clone(budget).try_acquire_many_owned(permits).map_err(|_| MachineOutputError::Overflow)
}

impl MachineSender {
    pub(super) fn publish(&self, chunk: &OutputChunk) -> bool {
        if self.sender.is_closed() {
            return false;
        }
        let result = chunk.bytes.len().checked_add(MACHINE_FRAME_OVERHEAD)
            .ok_or(MachineOutputError::Overflow)
            .and_then(|size| charge(&self.budget, size));
        match result {
            Ok(permit) => {
                let output = ChargedOutput { value: chunk.clone(), _charge: permit };
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
        hub.machine_senders.retain(|sender| !sender.sender.is_closed());
        hub.machine_senders.push(MachineSender {
            sender, status: status_tx, budget: Arc::clone(&budget),
        });
        let build = || {
            let controls = charge(&budget, MACHINE_CONTROL_BYTES)?;
            // Preflight retained bytes before allocating any replay copies.
            // A suffix may be smaller than retained history.
            let replay_bytes: usize = hub.buffer.chunks.iter()
                .filter(|chunk| after_sequence.is_none_or(|after| chunk.sequence > after))
                .map(|chunk| chunk.bytes.len()).sum();
            let permit = charge(&budget, replay_bytes.saturating_add(8 + MACHINE_FRAME_OVERHEAD))?;
            let (history, history_start_sequence, history_end_sequence, gap) =
                hub.buffer.snapshot_after(after_sequence);
            let gap = gap.or_else(|| hub.replay_gap.clone().filter(|gap| {
                after_sequence.is_none_or(|after| after < gap.available_from_sequence - 1)
            }));
            Ok(MachineAttachment {
                snapshot: ChargedOutput {
                    value: AttachmentSnapshot {
                        session_id: session_id.to_owned(), history_start_sequence,
                        history_end_sequence, history, history_segments: Vec::new(), gap,
                    },
                    _charge: permit,
                },
                receiver: MachineReceiver { receiver, status, budget: Arc::clone(&budget), _controls: controls },
            })
        };
        let result = build();
        if result.is_err() {
            hub.machine_senders.pop();
        }
        Some(result)
    }
}
