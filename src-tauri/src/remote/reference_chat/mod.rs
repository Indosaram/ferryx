//! Herdr reference-chat port. Ported from `devswha/herdr-web-ui` @
//! `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT, `docs/chat/HERDR_LICENSE`).
//! Frozen contract: `docs/chat/herdr-port-contract.md`.
//!
//! Registration rule for later lanes: the integration owner adds **one** `pub mod <name>;`
//! line for a completed module in the same change that adds its file, and never declares a
//! module whose file does not exist yet. Task 1 registers [`types`] only.

pub mod types;

pub use types::*;
