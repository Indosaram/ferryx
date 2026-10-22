//! Ferryx hub. It runs inside `ferryx-cli --daemon` and talks to panes through agent control.
//!
//! Slack, Discord, and voice compile behind `hub-slack`, `hub-discord`, and `hub-voice`.
//! The connectors stay idle until a token or speech key is configured.
#![deny(clippy::print_stdout)]

pub mod agent_control;
pub mod chat;
pub mod config;
pub mod context;
pub mod cron;
pub mod error;
pub mod fleet;
pub mod http;
pub mod hub;
pub mod import;
pub mod inbox;
pub mod judge;
pub mod lanes;
pub mod ledger;
pub mod mcp;
pub mod meta;
pub mod readiness;
pub mod schedule;
pub mod security;
pub mod sessions;
pub mod storage;
pub mod tracker;
pub mod tracker_linear;
pub mod voice;

pub use config::{secret_from_env, HubConfig};
pub use error::{HubError, Result};
pub use hub::Hub;
