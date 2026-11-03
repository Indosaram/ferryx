//! Optional audio source-time prefix, applied to the reassembled audio payload.
//! The timestamp uses the host session origin, exactly like TimestampStats::capture_us.
use crate::Capabilities;

pub fn audio_source_prefix(payload: &[u8], capture_us: u64, capabilities: Capabilities) -> Vec<u8> {
    if !capabilities.contains(Capabilities::AUDIO_SOURCE_TIME) {
        return payload.to_vec();
    }
    let mut output = Vec::with_capacity(payload.len() + 8);
    output.extend_from_slice(&capture_us.to_be_bytes());
    output.extend_from_slice(payload);
    output
}

pub fn split_audio_source_time(
    payload: &[u8],
    capabilities: Capabilities,
) -> Option<(Option<u64>, &[u8])> {
    if capabilities.contains(Capabilities::AUDIO_SOURCE_TIME) {
        let timestamp = u64::from_be_bytes(payload.get(..8)?.try_into().ok()?);
        Some((Some(timestamp), &payload[8..]))
    } else {
        Some((None, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamp_prefix_is_negotiated_independently_of_codec() {
        for caps in [
            Capabilities::empty(),
            Capabilities::AUDIO_OPUS,
            Capabilities::AUDIO_SOURCE_TIME,
            Capabilities::AUDIO_OPUS | Capabilities::AUDIO_SOURCE_TIME,
        ] {
            let encoded = audio_source_prefix(&[1, 2, 3], 123456, caps);
            let (time, data) = split_audio_source_time(&encoded, caps).unwrap();
            assert_eq!(data, &[1, 2, 3]);
            assert_eq!(
                time,
                caps.contains(Capabilities::AUDIO_SOURCE_TIME)
                    .then_some(123456)
            );
            if !caps.contains(Capabilities::AUDIO_SOURCE_TIME) {
                assert_eq!(encoded, [1, 2, 3]);
            }
        }
        assert!(split_audio_source_time(&[0; 7], Capabilities::AUDIO_SOURCE_TIME).is_none());
    }
}
