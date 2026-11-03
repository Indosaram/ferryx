//! Video loss recovery wire values (PERF-TRANSPORT-3), negotiated by
//! [`crate::Capabilities::FEC_NACK`].
//!
//! Parity is pure XOR over an interleaved stripe of one frame's data chunks:
//! parity `p` of `P` covers every chunk index `i` with `i % P == p`, so any
//! burst of up to `P` consecutive losses inside a frame is repairable. Each
//! parity also carries the XOR of the covered chunk lengths, so a recovered
//! chunk knows its exact size without any chunk-size convention.

use crate::{
    codec::{push_u16, push_u32, Decoder},
    CodecError, WireCodec, MAX_CHUNKS_PER_FRAME, MAX_VIDEO_CHUNK_BYTES,
};

/// Most indices one NACK may carry; keeps a NACK inside one MTU-safe datagram.
pub const MAX_NACK_INDICES: usize = 256;

/// Interleaved XOR parity for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameParity {
    pub frame_id: u32,
    /// Stripe offset `p`, `< parity_count`.
    pub parity_index: u16,
    /// Stripe count `P` for this frame, `>= 1`.
    pub parity_count: u16,
    /// XOR of the covered chunks' byte lengths.
    pub length_xor: u16,
    /// XOR of the covered chunks, each zero-padded to the longest.
    pub data: Vec<u8>,
}

impl FrameParity {
    pub const HEADER_SIZE: usize = 10;

    fn validate(&self) -> Result<(), CodecError> {
        if self.parity_count == 0 || self.parity_count > MAX_CHUNKS_PER_FRAME {
            return Err(CodecError::InvalidValue {
                field: "parity count",
                value: u64::from(self.parity_count),
            });
        }
        if self.parity_index >= self.parity_count {
            return Err(CodecError::InvalidValue {
                field: "parity index",
                value: u64::from(self.parity_index),
            });
        }
        if self.data.len() > MAX_VIDEO_CHUNK_BYTES {
            return Err(CodecError::LengthLimit {
                field: "parity data",
                actual: self.data.len(),
                max: MAX_VIDEO_CHUNK_BYTES,
            });
        }
        Ok(())
    }

    /// Chunk indices this parity covers in a frame of `total_chunks`.
    pub fn covered(&self, total_chunks: u16) -> impl Iterator<Item = u16> {
        (self.parity_index..total_chunks).step_by(usize::from(self.parity_count.max(1)))
    }
}

impl WireCodec for FrameParity {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(Self::HEADER_SIZE + self.data.len());
        push_u32(&mut output, self.frame_id);
        push_u16(&mut output, self.parity_index);
        push_u16(&mut output, self.parity_count);
        push_u16(&mut output, self.length_xor);
        output.extend_from_slice(&self.data);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            frame_id: decoder.u32("parity frame ID")?,
            parity_index: decoder.u16("parity index")?,
            parity_count: decoder.u16("parity count")?,
            length_xor: decoder.u16("parity length XOR")?,
            data: decoder.take_remaining().to_vec(),
        };
        value.validate()?;
        Ok(value)
    }
}

/// Stripe count rounded down so parity never exceeds the requested overhead.
/// Zero or non-finite redundancy disables parity.
pub fn parity_count_for(chunks: usize, redundancy: f64) -> u16 {
    if chunks == 0 || !redundancy.is_finite() || redundancy <= 0.0 {
        return 0;
    }
    let wanted = (chunks as f64 * redundancy.clamp(0.0, 1.0)).floor() as usize;
    wanted.min(chunks.min(MAX_CHUNKS_PER_FRAME as usize)) as u16
}

/// Builds every parity for `chunks` (indexed in frame order).
pub fn build_parity<T: AsRef<[u8]>>(
    frame_id: u32,
    chunks: &[T],
    parity_count: u16,
) -> Vec<FrameParity> {
    let stride = usize::from(parity_count);
    (0..parity_count)
        .map(|parity_index| {
            let covered = chunks
                .iter()
                .skip(usize::from(parity_index))
                .step_by(stride.max(1));
            let longest = covered
                .clone()
                .map(|chunk| chunk.as_ref().len())
                .max()
                .unwrap_or(0);
            let mut data = vec![0; longest];
            let mut length_xor = 0u16;
            for chunk in covered {
                let chunk = chunk.as_ref();
                xor_into(&mut data, chunk);
                length_xor ^= chunk.len() as u16;
            }
            FrameParity {
                frame_id,
                parity_index,
                parity_count,
                length_xor,
                data,
            }
        })
        .collect()
}

/// Recovers the single missing chunk of `parity`'s stripe from the present
/// ones. `None` when the stripe has zero or several holes, or when the
/// recovered length is inconsistent with the parity.
pub fn recover_chunk<'a>(
    parity: &FrameParity,
    total_chunks: u16,
    mut present: impl FnMut(u16) -> Option<&'a [u8]>,
) -> Option<(u16, Vec<u8>)> {
    let mut missing = None;
    let mut data = parity.data.clone();
    let mut length = parity.length_xor;
    for index in parity.covered(total_chunks) {
        match present(index) {
            Some(chunk) => {
                if chunk.len() > data.len() {
                    return None;
                }
                xor_into(&mut data, chunk);
                length ^= chunk.len() as u16;
            }
            None if missing.is_some() => return None,
            None => missing = Some(index),
        }
    }
    let index = missing?;
    let length = usize::from(length);
    if length == 0 || length > data.len() || data[length..].iter().any(|byte| *byte != 0) {
        return None;
    }
    data.truncate(length);
    Some((index, data))
}

fn xor_into(target: &mut [u8], source: &[u8]) {
    for (target, source) in target.iter_mut().zip(source) {
        *target ^= *source;
    }
}

/// Receiver feedback: chunks to retransmit plus the receiver's congestion
/// loss, which drives adaptive parity. An empty index list is a pure report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameNack {
    pub frame_id: u32,
    /// Receiver congestion loss in parts per thousand (0..=1000).
    pub loss_permille: u16,
    /// Measured receiver RTT rounded up to milliseconds. `u16::MAX` means
    /// unknown; it is never permission to retry without a transit budget.
    pub rtt_ms: u16,
    pub missing: Vec<u16>,
}

impl FrameNack {
    pub const HEADER_SIZE: usize = 10;

    fn validate(&self) -> Result<(), CodecError> {
        if self.loss_permille > 1000 {
            return Err(CodecError::InvalidValue {
                field: "NACK loss permille",
                value: u64::from(self.loss_permille),
            });
        }
        if self.missing.len() > MAX_NACK_INDICES {
            return Err(CodecError::LengthLimit {
                field: "NACK indices",
                actual: self.missing.len(),
                max: MAX_NACK_INDICES,
            });
        }
        if let Some(index) = self.missing.iter().find(|i| **i >= MAX_CHUNKS_PER_FRAME) {
            return Err(CodecError::InvalidValue {
                field: "NACK chunk index",
                value: u64::from(*index),
            });
        }
        Ok(())
    }
}

impl WireCodec for FrameNack {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.validate()?;
        let mut output = Vec::with_capacity(Self::HEADER_SIZE + self.missing.len() * 2);
        push_u32(&mut output, self.frame_id);
        push_u16(&mut output, self.loss_permille);
        push_u16(&mut output, self.rtt_ms);
        push_u16(&mut output, self.missing.len() as u16);
        for index in &self.missing {
            push_u16(&mut output, *index);
        }
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let frame_id = decoder.u32("NACK frame ID")?;
        let loss_permille = decoder.u16("NACK loss permille")?;
        let rtt_ms = decoder.u16("NACK RTT milliseconds")?;
        let count = usize::from(decoder.u16("NACK index count")?);
        if count > MAX_NACK_INDICES {
            return Err(CodecError::LengthLimit {
                field: "NACK indices",
                actual: count,
                max: MAX_NACK_INDICES,
            });
        }
        let missing = (0..count)
            .map(|_| decoder.u16("NACK chunk index"))
            .collect::<Result<Vec<_>, _>>()?;
        decoder.finish("frame NACK")?;
        let value = Self {
            frame_id,
            loss_permille,
            rtt_ms,
            missing,
        };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(count: usize, size: usize, last: usize) -> Vec<Vec<u8>> {
        (0..count)
            .map(|i| {
                let len = if i + 1 == count { last } else { size };
                (0..len).map(|b| (i * 31 + b * 7 + 1) as u8).collect()
            })
            .collect()
    }

    fn recover_all(
        parities: &[FrameParity],
        data: &[Vec<u8>],
        lost: &[bool],
    ) -> Vec<Option<Vec<u8>>> {
        let total = data.len() as u16;
        let mut have: Vec<Option<Vec<u8>>> = data
            .iter()
            .zip(lost)
            .map(|(chunk, lost)| (!lost).then(|| chunk.clone()))
            .collect();
        for parity in parities {
            let snapshot = have.clone();
            if let Some((index, bytes)) =
                recover_chunk(parity, total, |i| snapshot[usize::from(i)].as_deref())
            {
                have[usize::from(index)] = Some(bytes);
            }
        }
        have
    }

    /// Every loss pattern over small frames and every stripe count: a chunk
    /// is recovered exactly when it is the only hole in its stripe, and every
    /// recovery is byte-exact (including the short last chunk).
    #[test]
    fn xor_parity_recovers_exactly_single_holes_for_every_loss_pattern() {
        let mut patterns = 0usize;
        for count in 1..=8usize {
            for last in [1usize, 5, 9] {
                let data = chunks(count, 9, last);
                for parity_count in 1..=count as u16 {
                    let parities = build_parity(77, &data, parity_count);
                    assert_eq!(parities.len(), usize::from(parity_count));
                    for mask in 0u32..(1 << count) {
                        patterns += 1;
                        let lost: Vec<bool> = (0..count).map(|i| mask & (1 << i) != 0).collect();
                        let have = recover_all(&parities, &data, &lost);
                        for (index, chunk) in data.iter().enumerate() {
                            let stripe = index % usize::from(parity_count);
                            let holes = (0..count)
                                .filter(|i| i % usize::from(parity_count) == stripe && lost[*i])
                                .count();
                            if !lost[index] || holes == 1 {
                                assert_eq!(
                                    have[index].as_ref(),
                                    Some(chunk),
                                    "count {count} P {parity_count} mask {mask:b}"
                                );
                            } else {
                                assert_eq!(have[index], None);
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            patterns,
            3 * (1..=8).map(|n: u32| n << n).sum::<u32>() as usize
        );
    }

    #[test]
    fn burst_up_to_stripe_count_is_recoverable() {
        let data = chunks(40, 1150, 333);
        let parities = build_parity(1, &data, 4);
        for start in 0..37 {
            let lost: Vec<bool> = (0..40).map(|i| (start..start + 4).contains(&i)).collect();
            let have = recover_all(&parities, &data, &lost);
            assert!(have.iter().zip(&data).all(|(h, d)| h.as_ref() == Some(d)));
        }
    }

    #[test]
    fn corrupt_length_or_no_hole_recovers_nothing() {
        let data = chunks(3, 8, 3);
        let mut parity = build_parity(1, &data, 1).remove(0);
        assert_eq!(
            recover_chunk(&parity, 3, |i| Some(&data[usize::from(i)])),
            None
        );
        parity.length_xor ^= 0x40;
        assert_eq!(
            recover_chunk(&parity, 3, |i| (i != 1)
                .then(|| data[usize::from(i)].as_slice())),
            None
        );
    }

    #[test]
    fn parity_count_scales_with_redundancy() {
        assert_eq!(parity_count_for(0, 0.2), 0);
        assert_eq!(parity_count_for(1, 0.0), 0);
        assert_eq!(parity_count_for(4, 0.2), 0);
        assert_eq!(parity_count_for(10, f64::NAN), 0);
        assert_eq!(parity_count_for(10, f64::INFINITY), 0);
        assert_eq!(parity_count_for(10, 0.1), 1);
        assert_eq!(parity_count_for(10, 0.2), 2);
        assert_eq!(parity_count_for(10, 2.0), 10);
        assert_eq!(parity_count_for(100, 0.33), 33);
    }

    #[test]
    fn parity_and_nack_round_trip_and_reject_bad_values() {
        let parity = FrameParity {
            frame_id: 9,
            parity_index: 1,
            parity_count: 3,
            length_xor: 0x0102,
            data: vec![1, 2, 3],
        };
        let bytes = parity.encode().unwrap();
        assert_eq!(&bytes[..10], &[9, 0, 0, 0, 1, 0, 3, 0, 2, 1]);
        assert_eq!(FrameParity::decode(&bytes).unwrap(), parity);
        let mut bad = bytes.clone();
        bad[4] = 3;
        assert!(FrameParity::decode(&bad).is_err());
        bad[6] = 0;
        assert!(FrameParity::decode(&bad).is_err());

        let nack = FrameNack {
            frame_id: 5,
            loss_permille: 42,
            rtt_ms: 17,
            missing: vec![0, 7, 8191],
        };
        let bytes = nack.encode().unwrap();
        assert_eq!(
            bytes,
            [5, 0, 0, 0, 42, 0, 17, 0, 3, 0, 0, 0, 7, 0, 0xff, 0x1f]
        );
        assert_eq!(FrameNack::decode(&bytes).unwrap(), nack);
        assert!(FrameNack::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(FrameNack::decode(&trailing).is_err());
        let too_many = FrameNack {
            missing: vec![0; MAX_NACK_INDICES + 1],
            ..nack.clone()
        };
        assert!(too_many.encode().is_err());
        assert!(FrameNack {
            loss_permille: 1001,
            ..nack
        }
        .encode()
        .is_err());
    }
}
