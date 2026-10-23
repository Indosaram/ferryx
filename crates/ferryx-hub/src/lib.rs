//! Ferryx hub. It runs inside `ferryx-cli --daemon` and talks to panes through agent control.
//!
//! Slack, Discord, and voice compile behind `hub-slack`, `hub-discord`, and `hub-voice`.
//! The connectors stay idle until a token or speech key is configured.
#![deny(clippy::print_stdout)]

pub mod agent_control;
pub mod autonomy;
pub mod browser;
pub mod chat;
pub mod child_env;
pub mod config;
pub mod context;
pub mod control;
pub mod cron;
pub mod cron_exec;
pub mod cron_remediate;
pub mod dashboard_api;
pub mod discord_surface;
#[cfg(feature = "hub-discord")]
pub mod discord_voice;
pub mod effect;
pub mod error;
pub mod fleet;
pub mod http;
pub mod hub;
pub mod hub_env;
pub mod import;
pub mod inbox;
pub mod inbox_prefilter;
pub mod inbox_sources;
pub mod inbox_watch;
pub mod judge;
pub mod lanes;
pub mod ledger;
pub mod lifecycle;
pub mod mcp;
pub mod memory;
pub mod meta;
pub mod owner_alert;
pub mod readiness;
pub mod retention;
pub mod schedule;
pub mod security;
pub mod sessions;
pub mod storage;
pub mod table_image;
pub mod tools;
pub mod tracker;
pub mod tracker_lanes;
pub mod tracker_linear;
pub mod tracker_requests;
pub mod tracker_sessions;
pub mod voice;
pub mod voice_pcm;
pub mod voice_session;

pub use config::{secret_from_env, HubConfig};
pub use error::{HubError, Result};
pub use hub::Hub;
