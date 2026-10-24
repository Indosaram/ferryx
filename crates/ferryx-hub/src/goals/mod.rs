//! Supervisor goal contract loop: a per-lane goal with ID'd requirements, the evidence
//! Ferryx can vouch for, and the ledger of who drove each turn. Lanes without a contract
//! row are never touched here.
pub mod assess;
pub mod contract;
pub mod converge;
pub mod evidence;
/// The turn-by-turn driver. Tests live at `goals::loop_driver::tests`; callers may use the
/// `goals::r#loop` alias.
pub mod loop_driver;
pub mod prompts;
pub mod revision;
pub mod turns;
pub mod types;

pub use loop_driver as r#loop;

pub use types::{
    parse_model_decision, Decision, EvidenceSource, Gap, GapKind, GoalError, GoalMode,
    ProposalState, RequirementStatus, SendOutcome, StopReason, TurnOrigin, MODEL_STOP_REASONS,
};

/// The hub's pool type, so the daemon can call goal storage without its own sqlx dependency.
pub use sqlx::SqlitePool;
