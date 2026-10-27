use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::chat::router::ProfileRoute;
use crate::error::{HubError, Result};

/// Settings the hub reads from a JSON file plus `FERRYX_HUB_*` and token env vars.
///
/// The importer stays off until `FERRYX_HUB_IMPORT=1`. Discord and voice stay off unless
/// the crate was built with those features and a token is present.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HubConfig {
    pub database_path: Option<PathBuf>,
    /// Existing omo-gateway sqlite file. Read only when import is invoked.
    pub import_from: Option<PathBuf>,
    pub import_invoked: bool,
    pub supervisor_command: Vec<String>,
    pub supervisor_cwd: Option<PathBuf>,
    pub supervisor_listen_url: Option<String>,
    /// Launch the supervisor pane preset. The headless engine starts either way.
    pub supervisor_pane: bool,
    pub owner_slack_id: Option<String>,
    /// Slack channel for owner notices and approval cards (`SLACK_HOME_CHANNEL`).
    /// Absent means those posts go to the owner DM.
    pub slack_home_channel: Option<String>,
    /// Where owner-facing traffic (lead, digest, alerts) goes. omo-gateway reads
    /// `OMON_HOME_PLATFORM`; Discord is off here, so every value resolves to Slack.
    #[serde(default)]
    pub home_platform: Option<String>,
    pub tracker_addr: Option<String>,
    pub tracker_owner_token: Option<String>,
    pub meta_session_id: Option<String>,
    /// Profile routes. A route with `paneSessionId` sends that channel to a lane pane.
    #[serde(default)]
    pub profile_routes: Vec<ProfileRoute>,
    /// `TYPESAFE_*` values. The judge reads these and does not scrape `~/.zshenv`.
    pub typesafe: BTreeMap<String, String>,
}

impl HubConfig {
    /// File at `FERRYX_HUB_CONFIG`, then env overrides. Missing file is an empty config.
    pub fn load() -> Result<Self> {
        let mut config = match crate::hub_env::var("FERRYX_HUB_CONFIG") {
            Ok(path) => Self::from_file(Path::new(&path))?,
            Err(_) => Self::default(),
        };
        if let Ok(path) = crate::hub_env::var("FERRYX_HUB_DB") {
            config.database_path = Some(PathBuf::from(path));
        }
        if let Ok(path) = crate::hub_env::var("FERRYX_HUB_IMPORT_FROM") {
            config.import_from = Some(PathBuf::from(path));
        }
        if crate::hub_env::var("FERRYX_HUB_IMPORT").ok().as_deref() == Some("1") {
            config.import_invoked = true;
        }
        if crate::hub_env::var("FERRYX_HUB_SUPERVISOR_PANE")
            .ok()
            .as_deref()
            == Some("1")
        {
            config.supervisor_pane = true;
        }
        if let Ok(addr) = crate::hub_env::var("FERRYX_HUB_TRACKER_ADDR") {
            config.tracker_addr = Some(addr);
        }
        if let Ok(token) = crate::hub_env::var("FERRYX_HUB_TRACKER_TOKEN") {
            config.tracker_owner_token = Some(token);
        }
        if let Ok(id) = crate::hub_env::var("FERRYX_HUB_SLACK_OWNER") {
            config.owner_slack_id = Some(id);
        }
        if config
            .owner_slack_id
            .as_deref()
            .is_none_or(|id| id.trim().is_empty())
        {
            config.owner_slack_id = Some(slack_owner_from(
                crate::hub_env::var("SLACK_ALLOWED_USERS").ok().as_deref(),
            ));
        }
        config.apply_slack_home_override(crate::hub_env::var("SLACK_HOME_CHANNEL").ok().as_deref());
        config.home_platform = Some(
            home_platform(crate::hub_env::var("OMON_HOME_PLATFORM").ok().as_deref()).to_owned(),
        );
        if let Ok(url) = crate::hub_env::var("FERRYX_HUB_OMO_URL") {
            config.supervisor_listen_url = Some(url);
        }
        Ok(config)
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            HubError::config(format!(
                "cannot read hub config {}: {error}",
                path.display()
            ))
        })?;
        serde_json::from_str(&text)
            .map_err(|error| HubError::config(format!("hub config: {error}")))
    }

    /// `SLACK_HOME_CHANNEL` wins over the file. A blank value leaves the file alone.
    /// The first id is the channel, matching omo-gateway's list.
    pub fn apply_slack_home_override(&mut self, raw: Option<&str>) {
        if let Some(channel) = raw.and_then(normalize_slack_home) {
            self.slack_home_channel = Some(channel);
        }
    }
}

/// `OMON_HOME_PLATFORM`, resolved. omo-gateway sends owner traffic to Slack when the
/// value is `slack` or no Discord transport is configured. Discord is turned off in
/// ferryx (everything goes to Slack), so any value, `discord` included, is Slack.
pub fn home_platform(raw: Option<&str>) -> &'static str {
    if let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) {
        if !value.eq_ignore_ascii_case("slack") {
            tracing::info!(
                value,
                "OMON_HOME_PLATFORM names a platform other than Slack; Discord is off, using Slack"
            );
        }
    }
    "slack"
}

/// First Slack channel id in a comma-separated list. A leading `#` is dropped.
pub fn normalize_slack_home(raw: &str) -> Option<String> {
    let channel = raw
        .split(',')
        .map(str::trim)
        .find(|part| !part.is_empty())?
        .trim_start_matches('#')
        .trim()
        .to_owned();
    (!channel.is_empty()).then_some(channel)
}

/// Token from `NAME_FILE`, then `NAME`, then `~/.ferryx/hub/secrets/<name>` for the known
/// gateway files. A `*_FILE` value wins so a secret file is not ignored in favor of an
/// empty env var. The value itself is never logged.
pub fn secret_from_env(name: &str) -> Option<String> {
    let home = crate::hub_env::var_os("HOME").map(PathBuf::from);
    secret_from_lookup(
        name,
        |key| crate::hub_env::var(key).ok(),
        |path| std::fs::read_to_string(path).ok(),
        home.as_deref(),
    )
}

pub fn secret_from_lookup(
    name: &str,
    lookup: impl Fn(&str) -> Option<String>,
    read: impl Fn(&Path) -> Option<String>,
    home: Option<&Path>,
) -> Option<String> {
    let file_key = format!("{name}_FILE");
    if let Some(path) = lookup(&file_key) {
        let text = read(Path::new(path.trim()))?;
        let trimmed = text.trim().to_owned();
        if trimmed.is_empty() {
            return None;
        }
        return Some(trimmed);
    }
    if let Some(value) = lookup(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return Some(value);
    }
    let file = omon_secret_name(name)?;
    let home = home?;
    let text = read(&home.join(".ferryx").join("hub").join("secrets").join(file))?;
    let trimmed = text.trim().to_owned();
    if trimmed.is_empty() || !omon_prefix_ok(name, &trimmed) {
        return None;
    }
    Some(trimmed)
}

/// omo-gateway's owner: the first `SLACK_ALLOWED_USERS` entry, else its built-in owner id.
pub const DEFAULT_SLACK_OWNER_ID: &str = "U04M1JNJM8T";

pub fn slack_owner_from(allowed_users: Option<&str>) -> String {
    slack_allowed_from(allowed_users)
        .into_iter()
        .next()
        .unwrap_or_else(|| DEFAULT_SLACK_OWNER_ID.to_owned())
}

/// omo-gateway's `SLACK_ALLOWED_USERS` list (comma or whitespace separated).
pub fn slack_allowed_from(allowed_users: Option<&str>) -> Vec<String> {
    allowed_users
        .unwrap_or("")
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Who may press buttons and talk to the bot: `SLACK_ALLOWED_USERS` (else omo's owner) plus
/// the configured owner.
pub fn slack_allowed_users(owner: &str) -> Vec<String> {
    let mut users = slack_allowed_from(crate::hub_env::var("SLACK_ALLOWED_USERS").ok().as_deref());
    if users.is_empty() {
        users.push(DEFAULT_SLACK_OWNER_ID.to_owned());
    }
    let owner = owner.trim();
    if !owner.is_empty() && !users.iter().any(|id| id == owner) {
        users.push(owner.to_owned());
    }
    users
}

fn omon_secret_name(name: &str) -> Option<&'static str> {
    match name {
        "SLACK_BOT_TOKEN" => Some("slack-bot-token"),
        "SLACK_APP_TOKEN" => Some("slack-app-token"),
        "DISCORD_TOKEN" | "DISCORD_BOT_TOKEN" => Some("discord-bot-token"),
        "OPENAI_API_KEY" | "OMON_STT_API_KEY" | "OMON_TTS_API_KEY" => Some("openai-api-key"),
        _ => None,
    }
}

fn omon_prefix_ok(name: &str, value: &str) -> bool {
    match name {
        "SLACK_BOT_TOKEN" => value.starts_with("xoxb-"),
        "SLACK_APP_TOKEN" => value.starts_with("xapp-"),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_secret_wins_over_the_environment_variable() {
        let dir = tempfile_dir();
        let path = dir.join("token");
        std::fs::write(&path, "from-file\n").unwrap();
        // SAFETY: the test process is single-threaded for this env pair.
        unsafe {
            std::env::set_var("FERRYX_TEST_TOKEN_FILE", &path);
            std::env::set_var("FERRYX_TEST_TOKEN", "from-env");
        }
        assert_eq!(
            secret_from_env("FERRYX_TEST_TOKEN").as_deref(),
            Some("from-file")
        );
        unsafe {
            std::env::remove_var("FERRYX_TEST_TOKEN_FILE");
            std::env::remove_var("FERRYX_TEST_TOKEN");
        }
    }

    #[test]
    fn the_slack_owner_defaults_like_omo_gateway() {
        assert_eq!(slack_owner_from(None), "U04M1JNJM8T");
        assert_eq!(slack_owner_from(Some("  ")), "U04M1JNJM8T");
        assert_eq!(slack_owner_from(Some("UA, UB")), "UA");
        assert_eq!(slack_allowed_from(Some("UA,UB UC")), ["UA", "UB", "UC"]);
    }

    #[test]
    fn slack_home_channel_takes_the_first_id_and_a_blank_override_keeps_the_file() {
        assert_eq!(
            normalize_slack_home(" #C0C9XG5FP8W , COTHER ").as_deref(),
            Some("C0C9XG5FP8W")
        );
        assert_eq!(normalize_slack_home(" , ").as_deref(), None);
        let parsed: HubConfig =
            serde_json::from_str(r#"{"slackHomeChannel":" from-file "}"#).unwrap();
        assert_eq!(parsed.slack_home_channel.as_deref(), Some(" from-file "));
        let mut config = parsed;
        config.apply_slack_home_override(Some("  "));
        assert_eq!(config.slack_home_channel.as_deref(), Some(" from-file "));
        config.apply_slack_home_override(Some("CHOME,COTHER"));
        assert_eq!(config.slack_home_channel.as_deref(), Some("CHOME"));
    }

    #[test]
    fn a_hub_secret_file_is_used_when_the_environment_is_empty() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-secret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".ferryx/hub/secrets")).unwrap();
        std::fs::write(
            dir.join(".ferryx/hub/secrets/slack-bot-token"),
            "xoxb-from-file\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".ferryx/hub/secrets/slack-app-token"),
            "not-an-app-token\n",
        )
        .unwrap();
        let bot = secret_from_lookup(
            "SLACK_BOT_TOKEN",
            |_| None,
            |path| std::fs::read_to_string(path).ok(),
            Some(&dir),
        );
        assert_eq!(bot.as_deref(), Some("xoxb-from-file"));
        let app = secret_from_lookup(
            "SLACK_APP_TOKEN",
            |_| None,
            |path| std::fs::read_to_string(path).ok(),
            Some(&dir),
        );
        assert!(app.is_none());
        let env = secret_from_lookup(
            "SLACK_BOT_TOKEN",
            |key| (key == "SLACK_BOT_TOKEN").then(|| "xoxb-from-env".into()),
            |_| None,
            Some(&dir),
        );
        assert_eq!(env.as_deref(), Some("xoxb-from-env"));
    }

    fn tempfile_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("ferryx-hub-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn every_home_platform_resolves_to_slack() {
        for raw in [
            None,
            Some(""),
            Some("slack"),
            Some("Discord"),
            Some("discord"),
        ] {
            assert_eq!(home_platform(raw), "slack");
        }
    }
}
