//! The model a hub turn runs on when the hub starts it itself.
//!
//! Only the owner's own conversations (the owner DM and the Slack threads they talk in) run on
//! the session's model. Every turn the hub starts by itself runs on [`background_model`]: cron
//! agent jobs, lane events outside the owner DM, judge and kibitzer turns, and the retry after a
//! failed turn. A job payload or a session row that names another model does not change that,
//! so background work cannot drift onto the owner's model through stored data.

/// The background model when `OMON_BACKGROUND_MODEL` is unset.
pub const BACKGROUND_MODEL: &str = "mahoquot/gpt-6-luna";

/// `OMON_BACKGROUND_MODEL` from the daemon environment or the hub env file, else
/// [`BACKGROUND_MODEL`].
pub fn background_model() -> String {
    crate::hub_env::var("OMON_BACKGROUND_MODEL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| BACKGROUND_MODEL.to_owned())
}
