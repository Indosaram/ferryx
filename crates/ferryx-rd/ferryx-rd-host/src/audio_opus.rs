//! Shared host audio framing: 48kHz stereo, 10ms restricted-lowdelay Opus.
use opus::{Application, Bitrate, Channels, Encoder};

pub struct AudioOpusEncoder {
    encoder: Encoder,
    pending: Vec<f32>,
}

impl AudioOpusEncoder {
    pub fn new() -> Result<Self, opus::Error> {
        let mut encoder = Encoder::new(48_000, Channels::Stereo, Application::LowDelay)?;
        encoder.set_bitrate(Bitrate::Bits(160_000))?;
        Ok(Self {
            encoder,
            pending: Vec::with_capacity(960),
        })
    }

    pub fn buffered_frames(&self) -> usize {
        self.pending.len() / 2
    }

    pub fn encode_pcm(&mut self, pcm: &[u8]) -> Result<Vec<Vec<u8>>, opus::Error> {
        let mut packets = Vec::new();
        for sample in pcm.chunks_exact(4) {
            self.pending
                .push(f32::from_le_bytes(sample.try_into().expect("f32 bytes")));
            if self.pending.len() == 960 {
                let mut packet = vec![0; 1275];
                let len = self.encoder.encode_float(&self.pending, &mut packet)?;
                packet.truncate(len);
                packets.push(packet);
                self.pending.clear();
            }
        }
        Ok(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_capture_blocks_form_exact_ten_ms_packets() {
        let mut encoder = AudioOpusEncoder::new().unwrap();
        assert!(encoder.encode_pcm(&[0; 100 * 8]).unwrap().is_empty());
        assert_eq!(encoder.encode_pcm(&[0; 860 * 8]).unwrap().len(), 2);
        assert!(encoder.pending.is_empty());
    }
    #[test]
    fn round_trip_sine_matches_after_codec_lookahead() {
        let mut encoder = AudioOpusEncoder::new().unwrap();
        let delay = encoder.encoder.get_lookahead().unwrap() as usize;
        let mut decoder = opus::Decoder::new(48_000, Channels::Stereo).unwrap();
        let input: Vec<f32> = (0..480 * 20)
            .flat_map(|i| {
                let value = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.3;
                [value, value]
            })
            .collect();
        let bytes: Vec<u8> = input.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut decoded = Vec::new();
        for packet in encoder.encode_pcm(&bytes).unwrap() {
            let mut samples = [0.0; 960];
            assert_eq!(
                decoder.decode_float(&packet, &mut samples, false).unwrap(),
                480
            );
            decoded.extend_from_slice(&samples);
        }
        let compared = decoded.len() - delay * 2;
        let mse = input[..compared]
            .iter()
            .zip(&decoded[delay * 2..])
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            / compared as f32;
        assert!(mse < 0.002, "round-trip MSE {mse}");
    }
}
