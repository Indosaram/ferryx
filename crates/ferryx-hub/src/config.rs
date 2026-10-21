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
        let mut config = match std::env::var("FERRYX_HUB_CONFIG") {
            Ok(path) => Self::from_file(Path::new(&path))?,
            Err(_) => Self::default(),
        };
        if let Ok(path) = std::env::var("FERRYX_HUB_DB") {
            config.database_path = Some(PathBuf::from(path));
        }
        if let Ok(path) = std::env::var("FERRYX_HUB_IMPORT_FROM") {
            config.import_from = Some(PathBuf::from(path));
        }
        if std::env::var("FERRYX_HUB_IMPORT").ok().as_deref() == Some("1") {
            config.import_invoked = true;
        }
        if std::env::var("FERRYX_HUB_SUPERVISOR_PANE").ok().as_deref() == Some("1") {
            config.supervisor_pane = true;
        }
        if let Ok(addr) = std::env::var("FERRYX_HUB_TRACKER_ADDR") {
            config.tracker_addr = Some(addr);
        }
        if let Ok(token) = std::env::var("FERRYX_HUB_TRACKER_TOKEN") {
            config.tracker_owner_token = Some(token);
        }
        if let Ok(id) = std::env::var("FERRYX_HUB_SLACK_OWNER") {
            config.owner_slack_id = Some(id);
        }
        if let Ok(url) = std::env::var("FERRYX_HUB_OMO_URL") {
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
}

/// Token from `NAME`, or from the file named by `NAME_FILE` when that is set.
/// A `*_FILE` value wins so a secret file is not ignored in favor of an empty env var.
pub fn secret_from_env(name: &str) -> Option<String> {
    let file_key = format!("{name}_FILE");
    if let Ok(path) = std::env::var(&file_key) {
        let text = std::fs::read_to_string(&path).ok()?;
        let trimmed = text.trim().to_owned();
        if trimmed.is_empty() {
            return None;
        }
        return Some(trimmed);
    }
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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

    fn tempfile_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("ferryx-hub-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
