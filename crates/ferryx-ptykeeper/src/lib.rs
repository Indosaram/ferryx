//! Ferryx PTY keeper.
//!
//! The keeper owns `portable_pty` masters so a terminal session's child processes and their PTY
//! file descriptors survive an abnormal daemon exit. It is deliberately FFI-free: it never links
//! or calls `libghostty-vt` and never parses terminal output. Bytes are opaque here. Checkpoints
//! are produced by the daemon (which does parse) and transported by the keeper.
//!
//! Wire framing deliberately mirrors the daemon's handover wire (`daemon/handover_wire.rs`): the
//! same 60-byte header layout and the same `SCM_RIGHTS` attachment for descriptors. It is copied
//! rather than imported because the daemon is a Tauri library crate that this binary must not link.

pub mod budget;
pub mod checkpoint;
pub mod endpoint;
pub mod epoch;
pub mod error;
pub mod install;
pub mod keeper;
pub mod protocol;
pub mod replay;
pub mod session;

#[cfg(unix)]
pub mod server;

pub use budget::{MemoryBudget, BUDGET_AGGREGATE_BYTES, REPLAY_AGGREGATE_BYTES};
pub use checkpoint::{Checkpoint, CheckpointStore, CheckpointUpload};
pub use epoch::EpochFence;
pub use error::{KeeperError, KeeperErrorCode};
pub use keeper::Keeper;
pub use protocol::{Frame, Kind, PeerCredentials};
pub use replay::{ReplayGap, ReplayLog};
pub use session::SessionState;

/// Wire protocol major version. The protocol is additive-only within a major version: the daemon
/// must understand every keeper generation it may meet, so a new verb or payload field must not
/// change the meaning of an existing one.
pub const PROTOCOL_MAJOR: u16 = 1;
pub const PROTOCOL_MINOR: u16 = 0;

/// Reported through `HELLO_ACK` and recorded in the keeper's install directory name so a daemon
/// can tell generations apart without connecting twice.
pub const KEEPER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The per-session replay cap from the plan (8 MiB).
pub const REPLAY_SESSION_BYTES: usize = 8 * 1024 * 1024;

/// The one capability this keeper advertises in `HELLO_ACK`.
///
/// A daemon that sees it may rely on every destructive read naming an epoch: `REPLAY`, `HEAD` and
/// `CHECKPOINT_QUERY` all accept one and refuse an epoch a successor already replaced. The flag is
/// gated on the daemon side actually reading through the fenced forms, so it is advertised only
/// because `.omo/evidence/keeper-concurrency/REPLY-daemon-fenced-reads.md` records that audit.
pub const CAPABILITY_EPOCH_FENCED_CHECKPOINT: &str = "epoch-fenced-checkpoint-v1";
