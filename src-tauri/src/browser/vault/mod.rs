//! Browser password vault: an encrypted login store whose key lives in the OS keystore, login
//! capture and save prompts, exact-origin autofill, and a password generator.
//!
//! Provenance: ported from Maho (Project-Maho/maho-browser@c2830251,
//! maho/crates/maho-core vault_crypto, vault_manager/origin.rs, vault_generator and
//! maho/crates/maho-types vault types) under the owner's grant to reuse Maho code.
//!
//! Confidentiality boundary: the agent-facing surface never returns a stored secret. It writes
//! the secret into a page field the user already approved for that exact origin and reports only
//! a closed status token. This guarantee covers agents that do NOT hold an eval grant on that
//! origin: an eval-capable controller runs script in the page world, where a filled value is
//! readable, so the vault refuses such a controller outright (see
//! `AuthorityState::eval_blocks_vault`) instead of claiming general non-exfiltration.

pub mod autofill;
pub mod crypto;
pub mod generator;
pub mod grants;
pub mod provider;
pub mod keystore;
pub mod origin;
pub mod reauth;
pub mod store;

pub use autofill::{
    fill_for_page, fill_script, is_capture_envelope, parse_submission, PendingPrompts, SubmittedLogin,
    VaultSavePrompt, BROWSER_VAULT_SAVE_PROMPT_EVENT,
};
pub use origin::{credential_origin_matches, origin_of_page, CredentialOrigin};
pub use store::{VaultError, VaultItemSummary, VaultStore};

/// The vault's directory under the app data dir.
pub const VAULT_DIR: &str = "browser-vault";
