//! Supervisor goal contract loop: a per-lane goal with ID'd requirements, the evidence
//! Ferryx can vouch for, and the ledger of who drove each turn. Lanes without a contract
//! row are never touched here.
pub mod contract;
pub mod evidence;
pub mod turns;
pub mod types;

pub use types::{
    parse_model_decision, Decision, EvidenceSource, Gap, GapKind, GoalError, GoalMode,
    ProposalState, RequirementStatus, SendOutcome, StopReason, TurnOrigin, MODEL_STOP_REASONS,
};

/// The hub's pool type, so the daemon can call goal storage without its own sqlx dependency.
pub use sqlx::SqlitePool;
