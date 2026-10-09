pub mod frame;
pub mod messages;
pub mod types;
pub mod wire;

pub use frame::{decode_complete_frame, decode_frame, Decoded, FatalFrameError, Frame, Header};
pub use messages::{ErrorDetail, Message, PayloadError};
pub use types::{canonical_encode, state_digest, Uuid};
pub use wire::{Bytes, Codec, Digest, Reason};

pub const CAP_INPUT_LEASE: u64 = 1 << 0;
pub const CAP_RESIZE_LEASE: u64 = 1 << 1;
pub const CAP_SNAPSHOT_DELTA: u64 = 1 << 2;
pub const CAP_UI_EVENTS: u64 = 1 << 3;
pub const CAP_EPOCH_STATE: u64 = 1 << 4;
pub const CAP_V1_REQUIRED: u64 =
    CAP_INPUT_LEASE | CAP_RESIZE_LEASE | CAP_SNAPSHOT_DELTA | CAP_UI_EVENTS | CAP_EPOCH_STATE;

pub fn input_crc32c(bytes: &[u8]) -> u32 {
    crc32c::crc32c(bytes)
}
