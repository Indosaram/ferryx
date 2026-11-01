//! The model a hub turn runs on when the hub starts it itself.
//!
//! Only the owner's own conversations (the owner DM and the Slack threads they talk in) run on
//! the session's model. Every turn the hub starts by itself runs on [`background_model`]: cron
//! agent jobs, lane events outside the owner DM, judge and kibitzer turns, and the retry after a
//! failed turn. A job payload or a session row that names another model does not change that,
//! so background work cannot drift onto the owner's model through stored data.
//!
//! There is no built-in background model: with `OMON_BACKGROUND_MODEL` unset, background turns
//! run on the session's (or the engine's) default model.

/// `OMON_BACKGROUND_MODEL` (`provider/model`) from the daemon environment or the hub env file.
/// `None` (unset or blank) means "use the session/default model".
pub fn background_model() -> Option<String> {
    background_model_from(crate::hub_env::var("OMON_BACKGROUND_MODEL").ok().as_deref())
}

fn background_model_from(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_background_model_means_the_default_model() {
        assert_eq!(background_model_from(None), None);
        assert_eq!(background_model_from(Some("  ")), None);
        assert_eq!(
            background_model_from(Some(" provider/small ")).as_deref(),
            Some("provider/small")
        );
    }
}
