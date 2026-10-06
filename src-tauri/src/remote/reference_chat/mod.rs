//! Herdr reference-chat port. Ported from `devswha/herdr-web-ui` @
//! `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT, `docs/chat/HERDR_LICENSE`).
//! Frozen contract: `docs/chat/herdr-port-contract.md`.
//!
//! Registration rule for later lanes: the integration owner adds **one** `pub mod <name>;`
//! line for a completed module in the same change that adds its file, and never declares a
//! module whose file does not exist yet. Task 1 registered [`types`] only; task 13, the sole
//! backend integration owner, registers every authored lane below in the same change that
//! wires them onto the existing remote routes.

pub mod files;
pub mod history;
pub mod history_claude;
pub mod history_gjc;
pub mod history_omo;
pub mod history_omp;
pub mod history_parse;
pub mod history_pi;
pub mod input;
pub mod prompt_claude;
pub mod prompt_codex;
pub mod prompt_omo;
pub mod prompt_omp;
pub mod prompt_pi;
pub mod prompts;
pub mod screen;
pub mod types;

pub use types::*;

/// Route-composition regression source for the reference-chat routes (plan task 13).
#[cfg(test)]
#[path = "route_tests.rs"]
mod route_tests;
