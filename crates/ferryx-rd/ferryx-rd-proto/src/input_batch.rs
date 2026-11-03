//! Negotiated input datagrams and TCP barriers. Each epoch ends at a reliable
//! discrete event. Datagrams from future epochs are dropped, never applied ahead
//! of that event. A barrier repeats the epoch's cumulative motion, recovering UDP
//! loss without waiting for a missing datagram. Sequence numbers never wrap.
use crate::{CodecError, InputEvent, InputEventType, WireCodec};

pub const MAX_INPUT_SAMPLES: usize = 48;

pub fn is_pointer_motion(event: &InputEvent) -> bool {
    matches!(
        event.event_type,
        InputEventType::MouseMove
            | InputEventType::LeftMouseDragged
            | InputEventType::RightMouseDragged
            | InputEventType::RelativeMove
            | InputEventType::ScrollWheel
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputBatch {
    pub epoch: u64,
    pub sequence: u64,
    pub samples: Vec<InputEvent>,
}

impl WireCodec for InputBatch {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        if self.samples.len() > MAX_INPUT_SAMPLES {
            return Err(CodecError::LengthLimit {
                field: "input samples",
                actual: self.samples.len(),
                max: MAX_INPUT_SAMPLES,
            });
        }
        let mut out = Vec::with_capacity(18 + self.samples.len() * InputEvent::SIZE);
        out.extend_from_slice(&self.epoch.to_be_bytes());
        out.extend_from_slice(&self.sequence.to_be_bytes());
        out.extend_from_slice(&(self.samples.len() as u16).to_be_bytes());
        for event in &self.samples {
            out.extend(event.encode()?);
        }
        Ok(out)
    }
    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        if input.len() < 18 {
            return Err(CodecError::Truncated {
                field: "input batch",
                needed: 18,
                remaining: input.len(),
            });
        }
        let count = u16::from_be_bytes(input[16..18].try_into().unwrap()) as usize;
        if count > MAX_INPUT_SAMPLES {
            return Err(CodecError::LengthLimit {
                field: "input samples",
                actual: count,
                max: MAX_INPUT_SAMPLES,
            });
        }
        let expected = 18 + count * InputEvent::SIZE;
        if input.len() != expected {
            return Err(CodecError::InvalidLength {
                field: "input batch",
                actual: input.len(),
                expected,
            });
        }
        let samples = input[18..]
            .chunks_exact(InputEvent::SIZE)
            .map(InputEvent::decode)
            .collect::<Result<Vec<_>, _>>()?;
        if samples.iter().any(|e| {
            !is_pointer_motion(e)
                || ![e.x, e.y, e.scroll_dx, e.scroll_dy]
                    .into_iter()
                    .all(f32::is_finite)
        }) {
            return Err(CodecError::InvalidValue {
                field: "input motion",
                value: 0,
            });
        }
        Ok(Self {
            epoch: u64::from_be_bytes(input[..8].try_into().unwrap()),
            sequence: u64::from_be_bytes(input[8..16].try_into().unwrap()),
            samples,
        })
    }
}

/// A bounded reliable replay journal. Checkpoints are sent on TCP before the
/// journal reaches one datagram's sample budget, and before every discrete input.
#[derive(Default)]
pub struct InputJournal {
    pub epoch: u64,
    pub sequence: u64,
    pub pending: Vec<InputEvent>,
}
impl InputJournal {
    pub fn append(&mut self, samples: &[InputEvent]) -> InputBatch {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("input sequence exhausted");
        self.pending.extend_from_slice(samples);
        InputBatch {
            epoch: self.epoch,
            sequence: self.sequence,
            samples: self.pending.clone(),
        }
    }
    pub fn checkpoint(&mut self) -> InputBatch {
        let batch = InputBatch {
            epoch: self.epoch,
            sequence: self.sequence,
            samples: std::mem::take(&mut self.pending),
        };
        self.epoch = self.epoch.checked_add(1).expect("input epoch exhausted");
        batch
    }
}

/// Prefix snapshots let a newer datagram recover relative/scroll deltas lost in
/// earlier datagrams. Only the newest absolute sample is applied per snapshot.
#[derive(Default)]
pub struct InputOrder {
    epoch: u64,
    sequence: u64,
    applied: usize,
}
impl InputOrder {
    pub fn accept(&mut self, batch: &InputBatch, barrier: bool) -> Vec<InputEvent> {
        if batch.epoch != self.epoch {
            return Vec::new();
        }
        let mut out: Vec<InputEvent> = Vec::new();
        if batch.sequence > self.sequence {
            for event in batch.samples.iter().skip(self.applied) {
                if let Some(previous) = out
                    .last_mut()
                    .filter(|p| p.event_type == event.event_type && p.modifiers == event.modifiers)
                {
                    match event.event_type {
                        InputEventType::MouseMove
                        | InputEventType::LeftMouseDragged
                        | InputEventType::RightMouseDragged => {
                            *previous = *event;
                            continue;
                        }
                        InputEventType::RelativeMove | InputEventType::ScrollWheel => {
                            previous.scroll_dx += event.scroll_dx;
                            previous.scroll_dy += event.scroll_dy;
                            continue;
                        }
                        _ => {}
                    }
                }
                out.push(*event);
            }
            self.sequence = batch.sequence;
            self.applied = batch.samples.len();
        }
        if barrier {
            self.epoch += 1;
            self.applied = 0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn motion(kind: InputEventType, dx: f32) -> InputEvent {
        InputEvent {
            event_type: kind,
            x: dx,
            y: 0.5,
            key_code: 0,
            modifiers: crate::Modifiers::empty(),
            scroll_dx: dx,
            scroll_dy: 1.0,
        }
    }
    #[test]
    fn input_batch_codec_bounds_and_roundtrip() {
        let b = InputBatch {
            epoch: 2,
            sequence: 3,
            samples: vec![motion(InputEventType::MouseMove, 0.2)],
        };
        assert_eq!(InputBatch::decode(&b.encode().unwrap()).unwrap(), b);
        assert!(InputBatch::decode(&[0; 17]).is_err());
        let mut bytes = b.encode().unwrap();
        bytes[16..18].copy_from_slice(&49u16.to_be_bytes());
        assert!(InputBatch::decode(&bytes).is_err());
    }
    #[test]
    fn barrier_recovers_lost_motion_and_rejects_stale_and_future_epochs() {
        let mut sender = InputJournal::default();
        let mut host = InputOrder::default();
        let first = sender.append(&[motion(InputEventType::RelativeMove, 2.0)]);
        let second = sender.append(&[motion(InputEventType::RelativeMove, 3.0)]);
        assert_eq!(host.accept(&second, false)[0].scroll_dx, 5.0);
        assert!(host.accept(&first, false).is_empty());
        sender.append(&[motion(InputEventType::RelativeMove, 4.0)]);
        let barrier = sender.checkpoint();
        let future = sender.append(&[motion(InputEventType::RelativeMove, 7.0)]);
        assert!(host.accept(&future, false).is_empty());
        assert_eq!(host.accept(&barrier, true)[0].scroll_dx, 4.0);
        // Caller enqueues the discrete event here, before accepting epoch 1.
        assert_eq!(host.accept(&future, false)[0].scroll_dx, 7.0);
        assert!(host.accept(&second, false).is_empty());
    }
    #[test]
    fn scroll_loss_recovers_once_and_empty_discrete_epochs_advance() {
        let mut journal = InputJournal::default();
        let mut order = InputOrder::default();
        let lost = journal.append(&[motion(InputEventType::ScrollWheel, 2.0)]);
        let newest = journal.append(&[motion(InputEventType::ScrollWheel, 3.0)]);
        assert_eq!(order.accept(&newest, false)[0].scroll_dx, 5.0);
        assert!(order.accept(&lost, false).is_empty());
        assert!(order.accept(&journal.checkpoint(), true).is_empty());
        assert!(order.accept(&journal.checkpoint(), true).is_empty());
        let next = journal.append(&[motion(InputEventType::ScrollWheel, 7.0)]);
        assert_eq!(order.accept(&next, false)[0].scroll_dx, 7.0);
    }

    #[test]
    fn input_batch_rejects_discrete_nonfinite_and_oversized_payloads() {
        let mut b = InputBatch {
            epoch: 0,
            sequence: 1,
            samples: vec![motion(InputEventType::KeyDown, 0.5)],
        };
        assert!(InputBatch::decode(&b.encode().unwrap()).is_err());
        b.samples[0] = motion(InputEventType::MouseMove, f32::NAN);
        assert!(InputBatch::decode(&b.encode().unwrap()).is_err());
        b.samples = vec![motion(InputEventType::MouseMove, 0.5); MAX_INPUT_SAMPLES + 1];
        assert!(b.encode().is_err());
    }

    #[test]
    fn absolute_latest_and_relative_sum_per_batch() {
        let mut journal = InputJournal::default();
        let mut host = InputOrder::default();
        let batch = journal.append(&[
            motion(InputEventType::MouseMove, 0.1),
            motion(InputEventType::MouseMove, 0.9),
            motion(InputEventType::RelativeMove, 2.0),
            motion(InputEventType::RelativeMove, 3.0),
        ]);
        let events = host.accept(&batch, false);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].x, 0.9);
        assert_eq!(events[1].scroll_dx, 5.0);
        assert!(host.accept(&batch, false).is_empty());
    }
}
