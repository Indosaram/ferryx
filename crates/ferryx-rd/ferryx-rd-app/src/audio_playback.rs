//! Negotiated audio decoding. Legacy peers continue to send bare f32 LE PCM.
use ferryx_rd_decode::audio_opus::AudioOpusDecoder;

#[derive(Default)]
pub(crate) struct AudioPlayback {
    decoder: Option<AudioOpusDecoder>,
    continuity: crate::audio_continuity::AudioContinuity,
}

impl AudioPlayback {
    pub(crate) fn decode_frame(
        &mut self,
        id: u32,
        bytes: Vec<u8>,
        opus: bool,
    ) -> Result<Option<Vec<u8>>, std::io::Error> {
        let Some((missing, previous_bytes)) = self.continuity.missing(id, bytes.len()) else {
            return Ok(None);
        };
        let mut output = Vec::new();
        if opus {
            if self.decoder.is_none() {
                self.decoder = Some(AudioOpusDecoder::new().map_err(std::io::Error::other)?);
            }
            for _ in 0..missing {
                output.extend_from_slice(
                    &self
                        .decoder
                        .as_mut()
                        .expect("initialized decoder")
                        .conceal()
                        .map_err(std::io::Error::other)?,
                );
            }
        } else {
            // Legacy capture blocks may vary in size; use the last complete block's duration.
            output.resize(
                (missing as usize * previous_bytes).min(48_000 * 2 * 4 / 10),
                0,
            );
        }
        output.extend_from_slice(&self.decode(bytes, opus)?);
        Ok(Some(output))
    }

    pub(crate) fn decode(&mut self, bytes: Vec<u8>, opus: bool) -> Result<Vec<u8>, std::io::Error> {
        if !opus {
            return Ok(bytes);
        }
        if bytes.first() != Some(&1) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown negotiated audio codec",
            ));
        }
        if self.decoder.is_none() {
            self.decoder = Some(AudioOpusDecoder::new().map_err(std::io::Error::other)?);
        }
        self.decoder
            .as_mut()
            .expect("initialized decoder")
            .decode(&bytes[1..])
            .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_peer_pcm_is_not_interpreted_as_codec_header() {
        let pcm = vec![1, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            AudioPlayback::default().decode(pcm.clone(), false).unwrap(),
            pcm
        );
    }
    #[test]
    fn lost_pcm_block_inserts_silence_without_compressing_time() {
        let mut playback = AudioPlayback::default();
        let pcm = vec![4; 3840];
        assert_eq!(
            playback
                .decode_frame(1, pcm.clone(), false)
                .unwrap()
                .unwrap()
                .len(),
            3840
        );
        let output = playback
            .decode_frame(3, pcm.clone(), false)
            .unwrap()
            .unwrap();
        assert_eq!(output.len(), 7680);
        assert!(output[..3840].iter().all(|v| *v == 0));
        assert_eq!(&output[3840..], &pcm);
        assert!(playback.decode_frame(3, pcm, false).unwrap().is_none());
    }
    #[test]
    fn frame_gap_invokes_opus_plc_before_current_packet() {
        let mut playback = AudioPlayback::default();
        // A valid Opus silence packet for 10ms stereo CELT.
        let packet = vec![1, 0xf4, 0xff, 0xfe];
        let first = playback
            .decode_frame(1, packet.clone(), true)
            .unwrap()
            .unwrap();
        assert_eq!(first.len(), 3840);
        let after_gap = playback.decode_frame(3, packet, true).unwrap().unwrap();
        assert_eq!(after_gap.len(), 7680);
    }

    #[test]
    fn negotiated_codec_rejects_unknown_tag() {
        assert!(AudioPlayback::default().decode(vec![9], true).is_err());
    }
}
