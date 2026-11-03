use crate::{codec::usize_to_u32, CodecError};

pub const MAX_TCP_FRAME_SIZE: usize = 16 * 1024 * 1024;

pub struct TcpFrameWriter;

impl TcpFrameWriter {
    pub fn encode(payload: &[u8]) -> Result<Vec<u8>, CodecError> {
        if payload.is_empty() || payload.len() > MAX_TCP_FRAME_SIZE {
            return Err(CodecError::InvalidFrameLength(
                u32::try_from(payload.len()).unwrap_or(u32::MAX),
            ));
        }
        let length = usize_to_u32(payload.len(), "TCP frame")?;
        let mut output = Vec::with_capacity(4 + payload.len());
        output.extend_from_slice(&length.to_le_bytes());
        output.extend_from_slice(payload);
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpFrameEvent {
    Frame(Vec<u8>),
    DroppedInvalidLength(u32),
}

#[derive(Debug, Default)]
pub struct TcpFrameReader {
    buffer: Vec<u8>,
    #[cfg(test)]
    compaction_count: usize,
    #[cfg(test)]
    bytes_compacted: usize,
}

impl TcpFrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    #[cfg(test)]
    fn compaction_count(&self) -> usize {
        self.compaction_count
    }

    #[cfg(test)]
    fn bytes_compacted(&self) -> usize {
        self.bytes_compacted
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        #[cfg(test)]
        {
            self.compaction_count = 0;
            self.bytes_compacted = 0;
        }
    }

    pub fn push(&mut self, input: &[u8]) -> Vec<TcpFrameEvent> {
        self.buffer.extend_from_slice(input);
        let mut events = Vec::new();
        let mut offset = 0;

        while self.buffer.len() - offset >= 4 {
            let length = u32::from_le_bytes([
                self.buffer[offset],
                self.buffer[offset + 1],
                self.buffer[offset + 2],
                self.buffer[offset + 3],
            ]);
            if length == 0 || length as usize > MAX_TCP_FRAME_SIZE {
                self.buffer.clear();
                events.push(TcpFrameEvent::DroppedInvalidLength(length));
                return events;
            }
            let frame_end = 4 + length as usize;
            if self.buffer.len() - offset < frame_end {
                break;
            }
            let payload = self.buffer[offset + 4..offset + frame_end].to_vec();
            events.push(TcpFrameEvent::Frame(payload));
            offset += frame_end;
        }

        if offset == self.buffer.len() {
            self.buffer.clear();
        } else if offset > 0 {
            #[cfg(test)]
            let remaining = self.buffer.len() - offset;
            self.buffer.drain(..offset);
            #[cfg(test)]
            {
                self.compaction_count += 1;
                self.bytes_compacted += remaining;
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::{TcpFrameEvent, TcpFrameReader, TcpFrameWriter};

    #[test]
    fn test_burst_1024_coalesced_frames_plus_partial_tail_compaction_limit() {
        let mut reader = TcpFrameReader::new();

        // Prepare 1024 coalesced frames, each with a 60-byte payload (64 bytes total per frame).
        let mut coalesced = Vec::with_capacity(1024 * 64 + 7);
        let mut expected_payloads = Vec::with_capacity(1024);

        for i in 0..1024 {
            let payload = vec![(i % 251) as u8; 60];
            let encoded = TcpFrameWriter::encode(&payload).expect("frame encode must succeed");
            assert_eq!(encoded.len(), 64);
            coalesced.extend_from_slice(&encoded);
            expected_payloads.push(payload);
        }

        // Append 7 bytes of a 1025th frame (4-byte length prefix + 3 bytes of payload).
        let partial_tail = [0x10, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc];
        coalesced.extend_from_slice(&partial_tail);
        assert_eq!(coalesced.len(), 1024 * 64 + 7);

        // Push all 1024 coalesced frames and partial tail in a single push.
        let events = reader.push(&coalesced);

        // Assert that exactly 1024 frames were decoded.
        assert_eq!(events.len(), 1024);
        for (i, event) in events.iter().enumerate() {
            match event {
                TcpFrameEvent::Frame(payload) => {
                    assert_eq!(payload, &expected_payloads[i]);
                }
                TcpFrameEvent::DroppedInvalidLength(len) => {
                    panic!("unexpected dropped invalid length {len} at index {i}");
                }
            }
        }

        // Assert unparsed partial tail remains buffered.
        assert_eq!(reader.buffered_len(), 7);

        // Compaction proof: For 1024 coalesced frames with partial tail,
        // the reader MUST NOT perform repeated suffix compactions (O(N) memmoves).
        // An optimized reader does at most 1 compaction for the remaining partial tail.
        assert_eq!(
            reader.compaction_count(),
            1,
            "demonstrated repeated TCP suffix compaction regression: expected exactly 1 compaction, observed {}",
            reader.compaction_count()
        );
        assert_eq!(
            reader.bytes_compacted(),
            7,
            "expected exactly 7 bytes shifted (partial tail), observed {}",
            reader.bytes_compacted()
        );

        // Completing the tail must not cause another compaction.
        let mut tail_payload = vec![0xaa, 0xbb, 0xcc];
        tail_payload.extend_from_slice(&[0xdd; 13]);
        assert_eq!(
            reader.push(&[0xdd; 13]),
            vec![TcpFrameEvent::Frame(tail_payload)]
        );
        assert_eq!(reader.buffered_len(), 0);
        assert_eq!(reader.compaction_count(), 1);
        assert_eq!(reader.bytes_compacted(), 7);
    }

    #[test]
    fn test_coalesced_burst_without_tail_zero_compactions() {
        let mut reader = TcpFrameReader::new();

        let mut coalesced = Vec::with_capacity(1024 * 64);
        for i in 0..1024 {
            let payload = vec![(i % 251) as u8; 60];
            coalesced.extend_from_slice(&TcpFrameWriter::encode(&payload).unwrap());
        }

        let events = reader.push(&coalesced);
        assert_eq!(events.len(), 1024);
        assert_eq!(reader.buffered_len(), 0);

        // When the burst contains only complete frames, an optimized reader
        // clears the buffer with 0 compactions (zero memmoves).
        assert_eq!(
            reader.compaction_count(),
            0,
            "expected 0 compactions for complete coalesced burst, observed {} compactions",
            reader.compaction_count()
        );
        assert_eq!(
            reader.bytes_compacted(),
            0,
            "expected 0 bytes compacted for complete coalesced burst, observed {}",
            reader.bytes_compacted()
        );
    }
}
