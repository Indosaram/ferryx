use std::time::{Duration, Instant};

use ferryx_rd_proto::{
    ClipboardImageChunk, GuestPermissions, HostStatus, PairingTrust, SecureAttentionResponse,
    SessionEndReason, WireCodec,
};

use super::SessionError;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReconnectState {
    #[default]
    Idle,
    Reconnecting {
        attempt: u32,
        next_retry_ms: u64,
    },
    Reconnected,
    GaveUp {
        reason: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionStats {
    pub rtt_latest_ms: Option<f64>,
    pub rtt_smoothed_ms: Option<f64>,
    pub receive_bytes_per_second: Option<f64>,
    pub packet_loss_ratio: Option<f64>,
    pub decoder_backend: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionInfo {
    pub host_status: Option<HostStatus>,
    pub permissions: Option<GuestPermissions>,
    pub ended: Option<SessionEndReason>,
    pub secure_attention_response: Option<SecureAttentionResponse>,
    pub pairing_trust: Option<PairingTrust>,
    pub reconnect: ReconnectState,
}

pub(super) struct ParityState {
    pub info: SessionInfo,
    pub stats: SessionStats,
    pub epoch: Instant,
    pub probe: Option<(u64, Instant)>,
    pub sas_pending: Option<u32>,
    pub next_sas: u32,
    pub images: ImageAssembly,
    pub clipboard_enabled: bool,
    pub completed_image: Option<Vec<u8>>,
}

impl Default for ParityState {
    fn default() -> Self {
        Self {
            info: SessionInfo::default(),
            stats: SessionStats::default(),
            epoch: Instant::now(),
            probe: None,
            sas_pending: None,
            next_sas: 0,
            images: ImageAssembly::default(),
            clipboard_enabled: true,
            completed_image: None,
        }
    }
}

impl ParityState {
    pub fn pong(&mut self, timestamp: u64, now: Instant) {
        if let Some((expected, sent)) = self.probe {
            if expected == timestamp
                && now >= sent
                && now.duration_since(sent) <= Duration::from_secs(10)
            {
                let ms = now.duration_since(sent).as_secs_f64() * 1000.0;
                self.stats.rtt_latest_ms = Some(ms);
                self.stats.rtt_smoothed_ms = Some(
                    self.stats
                        .rtt_smoothed_ms
                        .map_or(ms, |old| old * 0.875 + ms * 0.125),
                );
                self.probe = None;
            }
        }
    }
}

/// A single bounded transfer. Monotonically increasing IDs prevent replay without
/// retaining an unbounded set of completed IDs for a long-lived connection.
#[derive(Default)]
pub(super) struct ImageAssembly {
    latest_id: Option<u32>,
    pending: Option<(u32, u32, Vec<u8>, Instant)>,
}

impl ImageAssembly {
    pub fn discard(&mut self) {
        self.pending = None;
    }

    pub fn push(
        &mut self,
        chunk: ClipboardImageChunk,
        now: Instant,
    ) -> Result<Option<Vec<u8>>, SessionError> {
        chunk.encode()?;
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, _, _, at)| now.duration_since(*at) >= Duration::from_secs(10))
        {
            self.discard();
        }
        if chunk.offset == 0 {
            if self.latest_id.is_some_and(|id| chunk.transfer_id <= id) {
                return Err(SessionError::InvalidClipboardImage);
            }
            self.latest_id = Some(chunk.transfer_id);
            self.pending = Some((
                chunk.transfer_id,
                chunk.total_bytes,
                Vec::with_capacity(chunk.total_bytes as usize),
                now,
            ));
        }
        let Some((id, total, data, _)) = self.pending.as_mut() else {
            return Err(SessionError::InvalidClipboardImage);
        };
        if *id != chunk.transfer_id
            || *total != chunk.total_bytes
            || data.len() != chunk.offset as usize
        {
            self.discard();
            return Err(SessionError::InvalidClipboardImage);
        }
        data.extend_from_slice(&chunk.png_bytes);
        if data.len() == *total as usize {
            let (_, _, data, _) = self.pending.take().expect("complete transfer exists");
            // The OS adapter receives only complete, decodable PNGs, with a
            // bounded native allocation even for compressed image bombs.
            let mut reader = image::ImageReader::with_format(
                std::io::Cursor::new(&data),
                image::ImageFormat::Png,
            );
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(8192);
            limits.max_image_height = Some(8192);
            limits.max_alloc = Some(64 * 1024 * 1024);
            reader.limits(limits);
            reader
                .decode()
                .map_err(|_| SessionError::InvalidClipboardImage)?;
            return Ok(Some(data));
        }
        Ok(None)
    }
}

pub(super) fn retry_delay(attempt: u32) -> Duration {
    Duration::from_millis(500 * (1_u64 << attempt.saturating_sub(1).min(4)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_pairing_defaults_to_owner() {
        let record = crate::PairingRecord::new("id", "host", vec![0; 32], 0);
        let mut json = serde_json::to_value(record).unwrap();
        json.as_object_mut().unwrap().remove("trust");
        let migrated: crate::PairingRecord = serde_json::from_value(json).unwrap();
        assert_eq!(migrated.trust, PairingTrust::Owner);
    }

    #[test]
    fn expiring_guest_pairing_retains_host_expiry() {
        let mut record = crate::PairingRecord::new("id", "host", vec![0; 32], 0);
        record.trust = PairingTrust::GuestExpiring {
            expires_at_unix_ms: 12345,
        };
        let saved = serde_json::to_vec(&record).unwrap();
        assert_eq!(
            serde_json::from_slice::<crate::PairingRecord>(&saved).unwrap(),
            record
        );
    }

    #[test]
    fn reconnect_backoff_is_bounded() {
        assert_eq!(retry_delay(1), Duration::from_millis(500));
        assert_eq!(retry_delay(99), Duration::from_secs(8));
    }

    #[test]
    fn rtt_requires_matching_outstanding_probe() {
        let mut state = ParityState::default();
        let now = state.epoch;
        state.probe = Some((1, now));
        state.pong(2, now + Duration::from_millis(20));
        assert_eq!(state.stats.rtt_latest_ms, None);
        state.pong(1, now + Duration::from_millis(20));
        assert_eq!(state.stats.rtt_latest_ms, Some(20.0));
        state.pong(1, now + Duration::from_millis(30));
        assert_eq!(state.stats.rtt_latest_ms, Some(20.0));
    }

    #[test]
    fn image_transfer_rejects_nonsequential_chunks() {
        let mut assembly = ImageAssembly::default();
        let now = Instant::now();
        assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: 16,
                    offset: 0,
                    png_bytes: ferryx_rd_proto::PNG_SIGNATURE.to_vec(),
                },
                now,
            )
            .unwrap();
        assert!(assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: 16,
                    offset: 9,
                    png_bytes: vec![0]
                },
                now
            )
            .is_err());
    }

    #[test]
    fn image_transfer_rejects_reused_id() {
        let mut assembly = ImageAssembly::default();
        let now = Instant::now();
        let chunk = ClipboardImageChunk {
            transfer_id: 1,
            total_bytes: 16,
            offset: 0,
            png_bytes: ferryx_rd_proto::PNG_SIGNATURE.to_vec(),
        };
        assembly.push(chunk.clone(), now).unwrap();
        assembly.discard();
        assert!(assembly.push(chunk, now).is_err());
    }

    #[test]
    fn image_transfer_delivers_only_complete_decodable_png() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let png = png.into_inner();
        let now = Instant::now();
        let mut assembly = ImageAssembly::default();
        assert_eq!(
            assembly
                .push(
                    ClipboardImageChunk {
                        transfer_id: 1,
                        total_bytes: png.len() as u32,
                        offset: 0,
                        png_bytes: png[..8].to_vec()
                    },
                    now
                )
                .unwrap(),
            None
        );
        let completed = assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: png.len() as u32,
                    offset: 8,
                    png_bytes: png[8..].to_vec(),
                },
                now,
            )
            .unwrap();
        assert_eq!(completed, Some(png));
    }

    #[test]
    fn image_transfer_expires_before_continuation() {
        let mut assembly = ImageAssembly::default();
        let now = Instant::now();
        assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: 16,
                    offset: 0,
                    png_bytes: ferryx_rd_proto::PNG_SIGNATURE.to_vec(),
                },
                now,
            )
            .unwrap();
        assert!(assembly
            .push(
                ClipboardImageChunk {
                    transfer_id: 1,
                    total_bytes: 16,
                    offset: 8,
                    png_bytes: vec![0; 8]
                },
                now + Duration::from_secs(10)
            )
            .is_err());
    }
}
