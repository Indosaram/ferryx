//! Cross-platform ferryx-rd client orchestration.

mod abr;
pub mod agent_input;
#[cfg(feature = "agent")]
pub mod agent_server;
mod audio;
mod audio_continuity;
mod audio_playback;
mod clipboard;
mod clock;
pub mod error;
pub mod frame_queue;
mod frame_jpeg;
mod input;
pub mod latency;
#[cfg(feature = "agent")]
pub mod mcp_server;
mod media;
mod pairing;
mod receiver_stats;
#[cfg(feature = "maho-relay")]
pub mod relay;
mod session;

pub use abr::*;
pub use agent_input::*;
#[cfg(feature = "agent")]
pub use agent_server::*;
pub use audio::*;
pub use clipboard::*;
pub use error::*;
pub use frame_jpeg::*;
pub use input::*;
pub use latency::*;
#[cfg(feature = "agent")]
pub use mcp_server::*;
pub use media::*;
pub use pairing::*;
pub use receiver_stats::{
    ReceiverSnapshot, RECEIVER_REORDER_GRACE, RECEIVER_STATS_WINDOW, RECEIVER_TELEMETRY_CAPACITY,
};
pub use session::*;
