use ferryx_rd_proto::ClipboardImageChunk;
use std::{
    collections::HashSet,
    io,
    time::{Duration, Instant},
};

pub(crate) const LIMIT: usize = 8 * 1024 * 1024;
const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Default)]
pub(crate) struct Assembly {
    active: Option<(u32, usize, Instant, Vec<u8>)>,
    used: HashSet<u32>,
}

impl Assembly {
    pub(crate) fn cancel(&mut self) {
        self.active = None;
    }

    pub(crate) fn expire(&mut self, now: Instant) {
        if self.active.as_ref().is_some_and(|(_, _, started, _)| {
            now.saturating_duration_since(*started) >= Duration::from_secs(5)
        }) {
            self.cancel();
        }
    }

    pub(crate) fn push(
        &mut self,
        chunk: ClipboardImageChunk,
        now: Instant,
    ) -> io::Result<Option<Vec<u8>>> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid clipboard image transfer",
            )
        };
        self.expire(now);
        let total = chunk.total_bytes as usize;
        if !(8..=LIMIT).contains(&total)
            || chunk.png_bytes.is_empty()
            || chunk.png_bytes.len() > 65536
        {
            self.cancel();
            return Err(invalid());
        }
        if chunk.offset == 0 {
            if self.used.len() >= 65536
                || !self.used.insert(chunk.transfer_id)
                || !chunk.png_bytes.starts_with(SIGNATURE)
            {
                self.cancel();
                return Err(invalid());
            }
            self.active = Some((chunk.transfer_id, total, now, Vec::with_capacity(total)));
        }
        let Some((id, expected, _, bytes)) = self.active.as_mut() else {
            return Err(invalid());
        };
        if *id != chunk.transfer_id
            || *expected != total
            || bytes.len() != chunk.offset as usize
            || bytes.len() + chunk.png_bytes.len() > total
        {
            self.cancel();
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk.png_bytes);
        if bytes.len() != total {
            return Ok(None);
        }
        let (_, _, _, bytes) = self.active.take().ok_or_else(invalid)?;
        validate_png(&bytes)?;
        Ok(Some(bytes))
    }
}

pub(crate) fn validate_png(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > LIMIT || !bytes.starts_with(SIGNATURE) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid PNG"));
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    let mut reader = decoder.read_info().map_err(io::Error::other)?;
    let size = reader.output_buffer_size();
    if size > 64 * 1024 * 1024 {
        return Err(io::Error::other("PNG decoded size exceeds limit"));
    }
    reader
        .next_frame(&mut vec![0; size])
        .map_err(io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 1, 1]).unwrap();
        }
        bytes
    }
    #[test]
    fn ordered_chunks_apply_only_on_completion() {
        let bytes = png();
        let mut assembly = Assembly::default();
        let now = Instant::now();
        assert!(assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: bytes.len() as u32,
                    offset: 0,
                    png_bytes: bytes[..8].to_vec()
                },
                now
            )
            .unwrap()
            .is_none());
        assert_eq!(
            assembly
                .push(
                    ClipboardImageChunk {
                        transfer_id: 1,
                        total_bytes: bytes.len() as u32,
                        offset: 8,
                        png_bytes: bytes[8..].to_vec()
                    },
                    now
                )
                .unwrap(),
            Some(bytes)
        );
    }
    #[test]
    fn gaps_and_reused_ids_are_rejected() {
        let mut assembly = Assembly::default();
        let now = Instant::now();
        let first = ClipboardImageChunk {
            transfer_id: 1,
            total_bytes: 20,
            offset: 0,
            png_bytes: SIGNATURE.to_vec(),
        };
        assembly.push(first.clone(), now).unwrap();
        assembly.cancel();
        assert!(assembly.push(first, now).is_err());
        assert!(assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 2,
                    total_bytes: 20,
                    offset: 8,
                    png_bytes: vec![0]
                },
                now
            )
            .is_err());
    }
    #[test]
    fn expired_partial_transfer_is_rejected() {
        let mut assembly = Assembly::default();
        let now = Instant::now();
        assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: 20,
                    offset: 0,
                    png_bytes: SIGNATURE.to_vec(),
                },
                now,
            )
            .unwrap();
        assert!(assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: 20,
                    offset: 8,
                    png_bytes: vec![0]
                },
                now + Duration::from_secs(5)
            )
            .is_err());
    }
}
