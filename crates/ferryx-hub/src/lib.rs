//! Ferryx hub. It runs inside `ferryx-cli --daemon` and talks to panes through agent control.
//!
//! Slack and Discord connectors compile behind `hub-slack` and `hub-discord`. Voice is a
//! feature flag with no implementation: huddle stays parked. SSH, the gateway tool loop,
//! the dashboard, inbox, and autonomy are not part of this crate.
#![deny(clippy::print_stdout)]

pub mod agent_control;
pub mod chat;
pub mod config;
pub mod cron;
pub mod error;
pub mod fleet;
pub mod hub;
pub mod import;
pub mod judge;
pub mod lanes;
pub mod mcp;
pub mod meta;
pub mod schedule;
pub mod security;
pub mod sessions;
pub mod storage;
pub mod tracker;

pub use config::{secret_from_env, HubConfig};
pub use error::{HubError, Result};
pub use hub::Hub;
