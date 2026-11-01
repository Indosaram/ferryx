//! 10ms stereo Opus decoder and packet-loss concealment.
pub struct AudioOpusDecoder {
    decoder: opus::Decoder,
    samples: [f32; 960],
}

impl AudioOpusDecoder {
    pub fn new() -> Result<Self, opus::Error> {
        Ok(Self {
            decoder: opus::Decoder::new(48_000, opus::Channels::Stereo)?,
            samples: [0.0; 960],
        })
    }
    pub fn decode(&mut self, packet: &[u8]) -> Result<Vec<u8>, opus::Error> {
        let frames = self
            .decoder
            .decode_float(packet, &mut self.samples, false)?;
        Ok(self.samples[..frames * 2]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect())
    }
    pub fn conceal(&mut self) -> Result<Vec<u8>, opus::Error> {
        // Empty input invokes libopus PLC. Restricted-lowdelay CELT has no SILK FEC.
        self.decode(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plc_on_gap_produces_exact_ten_ms_of_finite_stereo_pcm() {
        let mut encoder =
            opus::Encoder::new(48_000, opus::Channels::Stereo, opus::Application::LowDelay)
                .unwrap();
        let mut packet = [0; 1275];
        let size = encoder.encode_float(&[0.1; 960], &mut packet).unwrap();
        let mut decoder = AudioOpusDecoder::new().unwrap();
        assert_eq!(decoder.decode(&packet[..size]).unwrap().len(), 3840);
        let concealed = decoder.conceal().unwrap();
        assert_eq!(concealed.len(), 3840);
        assert!(concealed
            .chunks_exact(4)
            .all(|s| f32::from_le_bytes(s.try_into().unwrap()).is_finite()));
        assert_eq!(decoder.decode(&packet[..size]).unwrap().len(), 3840);
    }
}
