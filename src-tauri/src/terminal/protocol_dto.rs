use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use thiserror::Error;

pub const GHOSTTY_SNAPSHOT_MAGIC: &[u8; 8] = b"GHOSTSNP";
pub const GHOSTTY_SNAPSHOT_FORMAT_VERSION: u16 = 1;
pub const GHOSTTY_CLEAN_REVISION: &str = "6a508fd5e34c7e222c052a6d00bb3891ff3feace";
pub const ENGINE_IDENTIFIER: &str = "ghostty-vt";
pub const MAX_SNAPSHOT_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_COMPRESSED_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DECODED_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PASTE_BYTES: usize = 512 * 1024;
pub const MAX_RESIZE_COLS: u16 = 1000;
pub const MAX_RESIZE_ROWS: u16 = 1000;
pub const GHOSTTY_ENVELOPE_LEN: usize = 10;

const CRC32C_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0x82F6_3B78;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let index = ((crc ^ (byte as u32)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32C_TABLE[index];
    }
    crc ^ 0xFFFF_FFFFu32
}

pub mod base64_bytes {
    use super::*;

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = STANDARD.encode(bytes);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Base64Visitor;

        impl<'de> serde::de::Visitor<'de> for Base64Visitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a base64 string or byte sequence")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: DeError,
            {
                STANDARD.decode(value).map_err(DeError::custom)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut bytes = Vec::new();
                while let Some(byte) = seq.next_element()? {
                    bytes.push(byte);
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_any(Base64Visitor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SnapshotProtocolError {
    #[error("Snapshot envelope too short: expected at least {GHOSTTY_ENVELOPE_LEN} bytes, got {0}")]
    EnvelopeTooShort(usize),

    #[error("Invalid Ghostty snapshot magic bytes: expected 'GHOSTSNP', got {0:?}")]
    InvalidMagic([u8; 8]),

    #[error("Unsupported Ghostty snapshot version: expected {expected}, got {actual}")]
    UnsupportedVersion { expected: u16, actual: u16 },

    #[error("Incompatible terminal engine: expected '{expected}', got '{actual}'")]
    IncompatibleEngine { expected: String, actual: String },

    #[error("Incompatible engine source revision: expected '{expected}', got '{actual}'")]
    IncompatibleRevision { expected: String, actual: String },

    #[error("Chunk bytes ({actual}) exceeds maximum upper bound ({max})")]
    ChunkExceedsCap { actual: usize, max: usize },

    #[error("Total compressed payload size ({actual}) exceeds limit ({max})")]
    PayloadExceedsCap { actual: usize, max: usize },

    #[error("Declared decoded size ({actual}) exceeds 32MiB safety cap ({max})")]
    DecodedExceedsCap { actual: usize, max: usize },

    #[error("Paste payload size ({actual}) exceeds 512KiB staging cap ({max})")]
    PasteExceedsCap { actual: usize, max: usize },

    #[error("Total chunks must be greater than zero")]
    ZeroChunksDeclared,

    #[error("Snapshot ID cannot be blank")]
    BlankSnapshotId,

    #[error("Paste ID cannot be blank")]
    BlankPasteId,

    #[error("Session ID cannot be blank")]
    BlankSessionId,

    #[error("Lease ID cannot be blank")]
    BlankLeaseId,

    #[error("Invalid terminal dimensions: cols={cols}, rows={rows}")]
    InvalidDimensions { cols: u16, rows: u16 },

    #[error("Resize lease expired: expired_at={expired_at}, current_time={current_time}")]
    LeaseExpired { expired_at: u64, current_time: u64 },

    #[error("Resize lease signature or token cannot be blank")]
    MissingLeaseToken,

    #[error("Resize lease token rejected as unauthorized")]
    UnauthorizedLeaseToken,

    #[error("Stale generation: active generation {current} >= incoming {incoming}")]
    StaleGeneration { current: u64, incoming: u64 },

    #[error("Chunk index {index} out of bounds for total chunks {total}")]
    ChunkIndexOutOfBounds { index: u32, total: u32 },

    #[error("Duplicate chunk index {0} received with mismatched content")]
    ConflictingDuplicateChunk(u32),

    #[error("Chunk CRC32C mismatch for chunk {chunk_index}: expected {expected:#010X}, computed {computed:#010X}")]
    ChunkChecksumMismatch {
        chunk_index: u32,
        expected: u32,
        computed: u32,
    },

    #[error("Aggregate CRC32C mismatch: expected {expected:#010X}, computed {computed:#010X}")]
    AggregateChecksumMismatch { expected: u32, computed: u32 },

    #[error("Assembled snapshot byte length mismatch: declared {declared}, got {actual}")]
    LengthMismatch { declared: usize, actual: usize },

    #[error("Missing chunks at commit: received {received} of {total}")]
    IncompleteChunks { received: u32, total: u32 },

    #[error("Snapshot transfer was aborted: generation={generation}, reason={reason:?}")]
    TransferAborted {
        generation: u64,
        reason: SnapshotAbortReason,
    },

    #[error("No active snapshot transfer in progress")]
    NoActiveTransfer,

    #[error("Transfer snapshot ID mismatch: expected '{expected}', got '{actual}'")]
    SnapshotIdMismatch { expected: String, actual: String },

    #[error("Invalid input sequence number {0}: must be positive")]
    InvalidInputSequence(u64),

    #[error("Input payload size ({actual}) exceeds chunk limit ({max})")]
    InputExceedsCap { actual: usize, max: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineCompatibility {
    pub engine: String,
    pub source_revision: String,
    pub snapshot_version: u16,
}

impl Default for EngineCompatibility {
    fn default() -> Self {
        Self {
            engine: ENGINE_IDENTIFIER.to_string(),
            source_revision: GHOSTTY_CLEAN_REVISION.to_string(),
            snapshot_version: GHOSTTY_SNAPSHOT_FORMAT_VERSION,
        }
    }
}

impl EngineCompatibility {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn validate(&self) -> Result<(), SnapshotProtocolError> {
        if self.engine != ENGINE_IDENTIFIER {
            return Err(SnapshotProtocolError::IncompatibleEngine {
                expected: ENGINE_IDENTIFIER.to_string(),
                actual: self.engine.clone(),
            });
        }

        if self.snapshot_version != GHOSTTY_SNAPSHOT_FORMAT_VERSION {
            return Err(SnapshotProtocolError::UnsupportedVersion {
                expected: GHOSTTY_SNAPSHOT_FORMAT_VERSION,
                actual: self.snapshot_version,
            });
        }

        if !self.source_revision.eq_ignore_ascii_case(GHOSTTY_CLEAN_REVISION) {
            return Err(SnapshotProtocolError::IncompatibleRevision {
                expected: GHOSTTY_CLEAN_REVISION.to_string(),
                actual: self.source_revision.clone(),
            });
        }

        Ok(())
    }
}

pub fn validate_ghostty_snapshot_envelope(bytes: &[u8]) -> Result<u16, SnapshotProtocolError> {
    if bytes.len() < GHOSTTY_ENVELOPE_LEN {
        return Err(SnapshotProtocolError::EnvelopeTooShort(bytes.len()));
    }

    let mut magic = [0u8; 8];
    magic.copy_from_slice(&bytes[0..8]);
    if &magic != GHOSTTY_SNAPSHOT_MAGIC {
        return Err(SnapshotProtocolError::InvalidMagic(magic));
    }

    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != GHOSTTY_SNAPSHOT_FORMAT_VERSION {
        return Err(SnapshotProtocolError::UnsupportedVersion {
            expected: GHOSTTY_SNAPSHOT_FORMAT_VERSION,
            actual: version,
        });
    }

    Ok(version)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMetadata {
    pub snapshot_id: String,
    pub generation: u64,
    pub stream_epoch: u64,
    pub chunk_sequence: u64,
    pub engine_compat: EngineCompatibility,
    pub total_compressed_bytes: u32,
    pub total_decoded_bytes: u32,
    pub total_chunks: u32,
    pub has_unfinished_continuation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_rows_primary: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_rows_alternate: Option<u64>,
}

impl SnapshotMetadata {
    pub fn validate_pre_allocation(&self) -> Result<(), SnapshotProtocolError> {
        if self.snapshot_id.trim().is_empty() {
            return Err(SnapshotProtocolError::BlankSnapshotId);
        }

        self.engine_compat.validate()?;

        if self.total_chunks == 0 {
            return Err(SnapshotProtocolError::ZeroChunksDeclared);
        }

        let compressed_len = self.total_compressed_bytes as usize;
        if compressed_len > MAX_COMPRESSED_PAYLOAD_BYTES {
            return Err(SnapshotProtocolError::PayloadExceedsCap {
                actual: compressed_len,
                max: MAX_COMPRESSED_PAYLOAD_BYTES,
            });
        }

        let decoded_len = self.total_decoded_bytes as usize;
        if decoded_len > MAX_DECODED_BYTES {
            return Err(SnapshotProtocolError::DecodedExceedsCap {
                actual: decoded_len,
                max: MAX_DECODED_BYTES,
            });
        }

        let max_possible_chunks = (compressed_len.max(1) as u32).min(MAX_COMPRESSED_PAYLOAD_BYTES as u32);
        if self.total_chunks > max_possible_chunks {
            return Err(SnapshotProtocolError::PayloadExceedsCap {
                actual: self.total_chunks as usize,
                max: max_possible_chunks as usize,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub enum SnapshotAbortReason {
    #[serde(rename_all = "camelCase")]
    StaleGeneration {
        active_generation: u64,
        rejected_generation: u64,
    },
    #[serde(rename_all = "camelCase")]
    ChunkSequenceMismatch {
        expected_sequence: u64,
        actual_sequence: u64,
    },
    #[serde(rename_all = "camelCase")]
    LimitExceeded {
        limit: String,
        actual: usize,
        max: usize,
    },
    #[serde(rename_all = "camelCase")]
    ChecksumMismatch {
        expected: u32,
        computed: u32,
    },
    #[serde(rename_all = "camelCase")]
    IncompatibleEngine {
        detail: String,
    },
    ClientCancelled,
    UnknownSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SnapshotTransferMessage {
    #[serde(rename_all = "camelCase")]
    Begin {
        metadata: SnapshotMetadata,
    },
    #[serde(rename_all = "camelCase")]
    Chunk {
        snapshot_id: String,
        generation: u64,
        chunk_index: u32,
        #[serde(with = "base64_bytes")]
        chunk_bytes: Vec<u8>,
        chunk_crc32c: u32,
    },
    #[serde(rename_all = "camelCase")]
    Commit {
        snapshot_id: String,
        generation: u64,
        total_chunks: u32,
        total_compressed_bytes: u32,
        aggregate_crc32c: u32,
    },
    #[serde(rename_all = "camelCase")]
    Abort {
        snapshot_id: String,
        generation: u64,
        reason: SnapshotAbortReason,
    },
}

#[derive(Debug, Clone)]
pub struct SnapshotTransferAssembler {
    current_generation: u64,
    active_metadata: Option<SnapshotMetadata>,
    chunks: HashMap<u32, Vec<u8>>,
    received_bytes: usize,
}

impl Default for SnapshotTransferAssembler {
    fn default() -> Self {
        Self {
            current_generation: 0,
            active_metadata: None,
            chunks: HashMap::new(),
            received_bytes: 0,
        }
    }
}

impl SnapshotTransferAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_generation(&self) -> u64 {
        self.current_generation
    }

    pub fn active_metadata(&self) -> Option<&SnapshotMetadata> {
        self.active_metadata.as_ref()
    }

    pub fn reset(&mut self) {
        self.active_metadata = None;
        self.chunks.clear();
        self.received_bytes = 0;
    }

    pub fn handle_begin(&mut self, metadata: SnapshotMetadata) -> Result<(), SnapshotProtocolError> {
        metadata.validate_pre_allocation()?;

        if metadata.generation < self.current_generation {
            return Err(SnapshotProtocolError::StaleGeneration {
                current: self.current_generation,
                incoming: metadata.generation,
            });
        }

        self.current_generation = metadata.generation;
        self.active_metadata = Some(metadata);
        self.chunks.clear();
        self.received_bytes = 0;

        Ok(())
    }

    pub fn handle_chunk(
        &mut self,
        snapshot_id: &str,
        generation: u64,
        chunk_index: u32,
        chunk_bytes: Vec<u8>,
        chunk_crc32c: u32,
    ) -> Result<(), SnapshotProtocolError> {
        if chunk_bytes.len() > MAX_SNAPSHOT_CHUNK_BYTES {
            return Err(SnapshotProtocolError::ChunkExceedsCap {
                actual: chunk_bytes.len(),
                max: MAX_SNAPSHOT_CHUNK_BYTES,
            });
        }

        if generation < self.current_generation {
            return Err(SnapshotProtocolError::StaleGeneration {
                current: self.current_generation,
                incoming: generation,
            });
        }

        let meta = self
            .active_metadata
            .as_ref()
            .ok_or(SnapshotProtocolError::NoActiveTransfer)?;

        if generation != meta.generation {
            return Err(SnapshotProtocolError::StaleGeneration {
                current: meta.generation,
                incoming: generation,
            });
        }

        if snapshot_id != meta.snapshot_id {
            return Err(SnapshotProtocolError::SnapshotIdMismatch {
                expected: meta.snapshot_id.clone(),
                actual: snapshot_id.to_string(),
            });
        }

        if chunk_index >= meta.total_chunks {
            return Err(SnapshotProtocolError::ChunkIndexOutOfBounds {
                index: chunk_index,
                total: meta.total_chunks,
            });
        }

        let computed_crc = crc32c(&chunk_bytes);
        if computed_crc != chunk_crc32c {
            return Err(SnapshotProtocolError::ChunkChecksumMismatch {
                chunk_index,
                expected: chunk_crc32c,
                computed: computed_crc,
            });
        }

        if let Some(existing) = self.chunks.get(&chunk_index) {
            if existing != &chunk_bytes {
                return Err(SnapshotProtocolError::ConflictingDuplicateChunk(chunk_index));
            }
            return Ok(());
        }

        let new_total_bytes = self.received_bytes + chunk_bytes.len();
        if new_total_bytes > meta.total_compressed_bytes as usize {
            return Err(SnapshotProtocolError::PayloadExceedsCap {
                actual: new_total_bytes,
                max: meta.total_compressed_bytes as usize,
            });
        }

        self.received_bytes = new_total_bytes;
        self.chunks.insert(chunk_index, chunk_bytes);

        Ok(())
    }

    pub fn handle_commit(
        &mut self,
        snapshot_id: &str,
        generation: u64,
        total_chunks: u32,
        total_compressed_bytes: u32,
        aggregate_crc32c: u32,
    ) -> Result<Vec<u8>, SnapshotProtocolError> {
        if generation < self.current_generation {
            return Err(SnapshotProtocolError::StaleGeneration {
                current: self.current_generation,
                incoming: generation,
            });
        }

        let meta = self
            .active_metadata
            .as_ref()
            .ok_or(SnapshotProtocolError::NoActiveTransfer)?;

        if generation != meta.generation {
            return Err(SnapshotProtocolError::StaleGeneration {
                current: meta.generation,
                incoming: generation,
            });
        }

        if snapshot_id != meta.snapshot_id {
            return Err(SnapshotProtocolError::SnapshotIdMismatch {
                expected: meta.snapshot_id.clone(),
                actual: snapshot_id.to_string(),
            });
        }

        if total_chunks != meta.total_chunks {
            return Err(SnapshotProtocolError::IncompleteChunks {
                received: self.chunks.len() as u32,
                total: meta.total_chunks,
            });
        }

        if (self.chunks.len() as u32) != total_chunks {
            return Err(SnapshotProtocolError::IncompleteChunks {
                received: self.chunks.len() as u32,
                total: total_chunks,
            });
        }

        if (total_compressed_bytes as usize) != self.received_bytes {
            return Err(SnapshotProtocolError::LengthMismatch {
                declared: total_compressed_bytes as usize,
                actual: self.received_bytes,
            });
        }

        let mut assembled = Vec::with_capacity(self.received_bytes);
        for index in 0..total_chunks {
            let chunk = self
                .chunks
                .get(&index)
                .ok_or(SnapshotProtocolError::IncompleteChunks {
                    received: self.chunks.len() as u32,
                    total: total_chunks,
                })?;
            assembled.extend_from_slice(chunk);
        }

        let computed_aggregate_crc = crc32c(&assembled);
        if computed_aggregate_crc != aggregate_crc32c {
            return Err(SnapshotProtocolError::AggregateChecksumMismatch {
                expected: aggregate_crc32c,
                computed: computed_aggregate_crc,
            });
        }

        validate_ghostty_snapshot_envelope(&assembled)?;

        self.reset();

        Ok(assembled)
    }

    pub fn handle_abort(
        &mut self,
        snapshot_id: &str,
        generation: u64,
        reason: SnapshotAbortReason,
    ) -> Result<(), SnapshotProtocolError> {
        if generation < self.current_generation {
            return Err(SnapshotProtocolError::StaleGeneration {
                current: self.current_generation,
                incoming: generation,
            });
        }

        if let Some(meta) = &self.active_metadata {
            if meta.generation == generation && meta.snapshot_id == snapshot_id {
                self.reset();
                return Err(SnapshotProtocolError::TransferAborted { generation, reason });
            }
        }

        self.reset();
        Ok(())
    }
}

pub fn chunk_snapshot_payload(
    payload: &[u8],
    snapshot_id: &str,
    generation: u64,
) -> Result<(SnapshotMetadata, Vec<SnapshotTransferMessage>, SnapshotTransferMessage), SnapshotProtocolError> {
    if payload.len() > MAX_COMPRESSED_PAYLOAD_BYTES {
        return Err(SnapshotProtocolError::PayloadExceedsCap {
            actual: payload.len(),
            max: MAX_COMPRESSED_PAYLOAD_BYTES,
        });
    }

    validate_ghostty_snapshot_envelope(payload)?;

    let chunk_size = MAX_SNAPSHOT_CHUNK_BYTES;
    let chunks_slice = payload.chunks(chunk_size);
    let total_chunks = chunks_slice.len() as u32;

    let metadata = SnapshotMetadata {
        snapshot_id: snapshot_id.to_string(),
        generation,
        stream_epoch: 1,
        chunk_sequence: 1,
        engine_compat: EngineCompatibility::new(),
        total_compressed_bytes: payload.len() as u32,
        total_decoded_bytes: payload.len() as u32,
        total_chunks: total_chunks.max(1),
        has_unfinished_continuation: true,
        history_rows_primary: None,
        history_rows_alternate: None,
    };

    let mut chunk_messages = Vec::with_capacity(total_chunks as usize);
    for (i, slice) in payload.chunks(chunk_size).enumerate() {
        let chunk_crc = crc32c(slice);
        chunk_messages.push(SnapshotTransferMessage::Chunk {
            snapshot_id: snapshot_id.to_string(),
            generation,
            chunk_index: i as u32,
            chunk_bytes: slice.to_vec(),
            chunk_crc32c: chunk_crc,
        });
    }

    let aggregate_crc = crc32c(payload);
    let commit_message = SnapshotTransferMessage::Commit {
        snapshot_id: snapshot_id.to_string(),
        generation,
        total_chunks: total_chunks.max(1),
        total_compressed_bytes: payload.len() as u32,
        aggregate_crc32c: aggregate_crc,
    };

    Ok((metadata, chunk_messages, commit_message))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputMessage {
    pub input_epoch: u64,
    pub input_seq: u64,
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
}

impl InputMessage {
    pub fn validate(&self) -> Result<(), SnapshotProtocolError> {
        if self.input_seq == 0 {
            return Err(SnapshotProtocolError::InvalidInputSequence(0));
        }

        if self.data.len() > MAX_SNAPSHOT_CHUNK_BYTES {
            return Err(SnapshotProtocolError::InputExceedsCap {
                actual: self.data.len(),
                max: MAX_SNAPSHOT_CHUNK_BYTES,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteMessage {
    pub input_epoch: u64,
    pub paste_id: String,
    pub paste_seq: u64,
    pub bracketed: bool,
    #[serde(with = "base64_bytes")]
    pub payload: Vec<u8>,
}

impl PasteMessage {
    pub fn validate_staging_cap(&self) -> Result<(), SnapshotProtocolError> {
        if self.paste_id.trim().is_empty() {
            return Err(SnapshotProtocolError::BlankPasteId);
        }

        if self.payload.len() > MAX_PASTE_BYTES {
            return Err(SnapshotProtocolError::PasteExceedsCap {
                actual: self.payload.len(),
                max: MAX_PASTE_BYTES,
            });
        }

        Ok(())
    }

    pub fn to_terminal_bytes(&self) -> Vec<u8> {
        if self.bracketed {
            let mut out = Vec::with_capacity(self.payload.len() + 12);
            out.extend_from_slice(b"\x1b[200~");
            out.extend_from_slice(&self.payload);
            out.extend_from_slice(b"\x1b[201~");
            out
        } else {
            self.payload.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeLeaseMessage {
    pub session_id: String,
    pub lease_id: String,
    pub lease_epoch: u64,
    pub chunk_sequence: u64,
    pub cols: u16,
    pub rows: u16,
    pub signature_or_token: String,
    pub expires_at_unix_millis: u64,
}

impl ResizeLeaseMessage {
    pub fn validate(
        &self,
        current_time_millis: u64,
        expected_token: Option<&str>,
    ) -> Result<(), SnapshotProtocolError> {
        if self.session_id.trim().is_empty() {
            return Err(SnapshotProtocolError::BlankSessionId);
        }

        if self.lease_id.trim().is_empty() {
            return Err(SnapshotProtocolError::BlankLeaseId);
        }

        if self.cols == 0 || self.cols > MAX_RESIZE_COLS || self.rows == 0 || self.rows > MAX_RESIZE_ROWS {
            return Err(SnapshotProtocolError::InvalidDimensions {
                cols: self.cols,
                rows: self.rows,
            });
        }

        if self.signature_or_token.trim().is_empty() {
            return Err(SnapshotProtocolError::MissingLeaseToken);
        }

        if current_time_millis > self.expires_at_unix_millis {
            return Err(SnapshotProtocolError::LeaseExpired {
                expired_at: self.expires_at_unix_millis,
                current_time: current_time_millis,
            });
        }

        if let Some(token) = expected_token {
            if self.signature_or_token != token {
                return Err(SnapshotProtocolError::UnauthorizedLeaseToken);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARED_FIXTURE_JSON: &str =
        include_str!("../../../ui/src/lib/__fixtures__/terminalProtocol.fixtures.json");

    #[test]
    fn crc32c_matches_rfc3720_test_vector() {
        let vector = b"123456789";
        assert_eq!(crc32c(vector), 0xE3069283);
        assert_eq!(crc32c(b""), 0);
    }

    #[test]
    fn envelope_validation_enforces_magic_and_version() {
        let valid = b"GHOSTSNP\x01\x00extra_payload";
        assert_eq!(validate_ghostty_snapshot_envelope(valid).unwrap(), 1);

        let bad_magic = b"NOTMAGIC\x01\x00";
        assert!(matches!(
            validate_ghostty_snapshot_envelope(bad_magic),
            Err(SnapshotProtocolError::InvalidMagic(_))
        ));

        let bad_version = b"GHOSTSNP\x02\x00";
        assert!(matches!(
            validate_ghostty_snapshot_envelope(bad_version),
            Err(SnapshotProtocolError::UnsupportedVersion { expected: 1, actual: 2 })
        ));

        let short = b"GHOST";
        assert!(matches!(
            validate_ghostty_snapshot_envelope(short),
            Err(SnapshotProtocolError::EnvelopeTooShort(5))
        ));
    }

    #[test]
    fn pre_allocation_rejects_impossible_limits() {
        let oversized_compressed = SnapshotMetadata {
            snapshot_id: "id1".to_string(),
            generation: 1,
            stream_epoch: 1,
            chunk_sequence: 1,
            engine_compat: EngineCompatibility::new(),
            total_compressed_bytes: (MAX_COMPRESSED_PAYLOAD_BYTES + 1) as u32,
            total_decoded_bytes: 1024,
            total_chunks: 1,
            has_unfinished_continuation: true,
            history_rows_primary: None,
            history_rows_alternate: None,
        };
        assert!(matches!(
            oversized_compressed.validate_pre_allocation(),
            Err(SnapshotProtocolError::PayloadExceedsCap { .. })
        ));

        let oversized_decoded = SnapshotMetadata {
            total_compressed_bytes: 1024,
            total_decoded_bytes: (MAX_DECODED_BYTES + 1) as u32,
            ..oversized_compressed.clone()
        };
        assert!(matches!(
            oversized_decoded.validate_pre_allocation(),
            Err(SnapshotProtocolError::DecodedExceedsCap { .. })
        ));

        let zero_chunks = SnapshotMetadata {
            total_chunks: 0,
            ..oversized_compressed
        };
        assert!(matches!(
            zero_chunks.validate_pre_allocation(),
            Err(SnapshotProtocolError::ZeroChunksDeclared)
        ));
    }

    #[test]
    fn multi_chunk_transfer_and_stale_generation_rules() {
        let mut assembler = SnapshotTransferAssembler::new();

        let meta = SnapshotMetadata {
            snapshot_id: "snap-1".to_string(),
            generation: 5,
            stream_epoch: 1,
            chunk_sequence: 10,
            engine_compat: EngineCompatibility::new(),
            total_compressed_bytes: 24,
            total_decoded_bytes: 100,
            total_chunks: 2,
            has_unfinished_continuation: true,
            history_rows_primary: None,
            history_rows_alternate: None,
        };

        assembler.handle_begin(meta).unwrap();
        assert_eq!(assembler.current_generation(), 5);

        let stale_meta = SnapshotMetadata {
            generation: 4,
            ..assembler.active_metadata().unwrap().clone()
        };
        assert!(matches!(
            assembler.handle_begin(stale_meta),
            Err(SnapshotProtocolError::StaleGeneration { current: 5, incoming: 4 })
        ));

        let chunk0_bytes = b"GHOSTSNP\x01\x0012".to_vec();
        let chunk0_crc = crc32c(&chunk0_bytes);
        assembler
            .handle_chunk("snap-1", 5, 0, chunk0_bytes.clone(), chunk0_crc)
            .unwrap();

        assert!(matches!(
            assembler.handle_chunk("snap-1", 4, 1, vec![1, 2], 123),
            Err(SnapshotProtocolError::StaleGeneration { current: 5, incoming: 4 })
        ));

        assert!(matches!(
            assembler.handle_chunk("snap-1", 5, 1, vec![1, 2, 3], 999999),
            Err(SnapshotProtocolError::ChunkChecksumMismatch { .. })
        ));

        let chunk1_bytes = b"34567890ABCD".to_vec();
        let chunk1_crc = crc32c(&chunk1_bytes);
        assembler
            .handle_chunk("snap-1", 5, 1, chunk1_bytes.clone(), chunk1_crc)
            .unwrap();

        let mut expected_payload = Vec::new();
        expected_payload.extend_from_slice(&chunk0_bytes);
        expected_payload.extend_from_slice(&chunk1_bytes);
        let aggregate_crc = crc32c(&expected_payload);

        let assembled = assembler
            .handle_commit("snap-1", 5, 2, 24, aggregate_crc)
            .unwrap();
        assert_eq!(assembled, expected_payload);
    }

    #[test]
    fn paste_message_enforces_512kib_staging_cap() {
        let valid_payload = vec![b'a'; MAX_PASTE_BYTES];
        let valid_msg = PasteMessage {
            input_epoch: 1,
            paste_id: "paste-1".to_string(),
            paste_seq: 1,
            bracketed: true,
            payload: valid_payload,
        };
        assert!(valid_msg.validate_staging_cap().is_ok());

        let oversized_msg = PasteMessage {
            payload: vec![b'a'; MAX_PASTE_BYTES + 1],
            ..valid_msg
        };
        assert!(matches!(
            oversized_msg.validate_staging_cap(),
            Err(SnapshotProtocolError::PasteExceedsCap { actual, max }) if actual == 524289 && max == 524288
        ));
    }

    #[test]
    fn authenticated_resize_lease_validation() {
        let msg = ResizeLeaseMessage {
            session_id: "sess-1".to_string(),
            lease_id: "lease-1".to_string(),
            lease_epoch: 1,
            chunk_sequence: 100,
            cols: 80,
            rows: 24,
            signature_or_token: "secret-token".to_string(),
            expires_at_unix_millis: 10_000,
        };

        assert!(msg.validate(5_000, Some("secret-token")).is_ok());

        assert!(matches!(
            msg.validate(15_000, Some("secret-token")),
            Err(SnapshotProtocolError::LeaseExpired { .. })
        ));

        assert!(matches!(
            msg.validate(5_000, Some("wrong-token")),
            Err(SnapshotProtocolError::UnauthorizedLeaseToken)
        ));

        let zero_cols = ResizeLeaseMessage { cols: 0, ..msg.clone() };
        assert!(matches!(
            zero_cols.validate(5_000, None),
            Err(SnapshotProtocolError::InvalidDimensions { .. })
        ));
    }

    #[test]
    fn shared_fixture_json_roundtrip() {
        let parsed: serde_json::Value =
            serde_json::from_str(SHARED_FIXTURE_JSON).expect("valid fixture JSON");

        let constants = &parsed["constants"];
        assert_eq!(constants["ghosttySnapshotMagic"].as_str().unwrap(), "GHOSTSNP");
        assert_eq!(constants["ghosttySnapshotFormatVersion"].as_u64().unwrap(), 1);
        assert_eq!(
            constants["ghosttyCleanRevision"].as_str().unwrap(),
            GHOSTTY_CLEAN_REVISION
        );
        assert_eq!(constants["maxSnapshotChunkBytes"].as_u64().unwrap(), 65536);
        assert_eq!(constants["maxPasteBytes"].as_u64().unwrap(), 524288);

        let env_hex = parsed["envelope"]["hex"].as_str().unwrap();
        let mut env_bytes = Vec::new();
        for i in (0..env_hex.len()).step_by(2) {
            env_bytes.push(u8::from_str_radix(&env_hex[i..i + 2], 16).unwrap());
        }
        assert_eq!(validate_ghostty_snapshot_envelope(&env_bytes).unwrap(), 1);
        assert_eq!(crc32c(&env_bytes), parsed["envelope"]["crc32c"].as_u64().unwrap() as u32);
    }
}
