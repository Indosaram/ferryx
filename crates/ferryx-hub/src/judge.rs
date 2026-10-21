//! typesafe-judge, then the omo fallback. `TYPESAFE_*` comes from hub config, never `~/.zshenv`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{HubError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    DoneDiff,
    OwnerIntent,
    EventWake,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub selected: String,
    pub is_fallback: bool,
}

impl Choice {
    pub fn new(selected: impl Into<String>) -> Self {
        Self {
            selected: selected.into(),
            is_fallback: false,
        }
    }

    pub fn fallback(selected: impl Into<String>) -> Self {
        Self {
            selected: selected.into(),
            is_fallback: true,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.selected
    }
}

/// Replaces a home directory and obvious token shapes before text leaves the machine.
pub fn scrub_for_external(input: &str) -> String {
    let mut scrubbed = input.to_owned();
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            scrubbed = scrubbed.replace(&home, "~");
        }
    }
    if let Ok(pattern) = regex::Regex::new(r"(?i)\b(sk-|ghp_|xox[baprs]-)[A-Za-z0-9_\-]{8,}") {
        scrubbed = pattern.replace_all(&scrubbed, "[redacted]").into_owned();
    }
    scrubbed.chars().take(4000).collect()
}

pub fn typesafe_env(config: &BTreeMap<String, String>) -> Vec<(String, String)> {
    const KEYS: [&str; 3] = ["TYPESAFE_API_KEY", "TYPESAFE_URL", "TYPESAFE_MODEL"];
    KEYS.iter()
        .filter_map(|key| {
            config
                .get(*key)
                .map(|value| ((*key).to_owned(), value.clone()))
        })
        .collect()
}

pub fn default_judge_bin() -> PathBuf {
    let on_path = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| dir.join("typesafe-judge").is_file())
    });
    if !on_path {
        if let Some(home) = std::env::var_os("HOME") {
            let cargo = Path::new(&home).join(".cargo/bin/typesafe-judge");
            if cargo.is_file() {
                return cargo;
            }
        }
    }
    PathBuf::from("typesafe-judge")
}

#[derive(Clone)]
pub struct Judge {
    typesafe: BTreeMap<String, String>,
    /// When set, a missing binary uses this choice instead of spawning a model.
    offline_fallback: Option<String>,
}

impl Judge {
    pub fn from_config(typesafe: BTreeMap<String, String>) -> Self {
        Self {
            typesafe,
            offline_fallback: None,
        }
    }

    /// Offline fallback used when typesafe-judge is not installed. Tests and a hub
    /// with no judge binary still settle a claim whose signals already passed.
    pub fn fallback_yes() -> Self {
        Self {
            typesafe: BTreeMap::new(),
            offline_fallback: Some("yes".into()),
        }
    }

    pub async fn choice(
        &self,
        _purpose: Purpose,
        state: &str,
        instructions: &str,
        options: &[&str],
    ) -> Result<Choice> {
        let _ = scrub_for_external(state);
        match self.run_typesafe(instructions, options).await {
            Ok(choice) => Ok(choice),
            Err(error) => {
                if let Some(selected) = &self.offline_fallback {
                    if options.contains(&selected.as_str()) {
                        return Ok(Choice::fallback(selected.clone()));
                    }
                }
                Err(error)
            }
        }
    }

    async fn run_typesafe(&self, instructions: &str, options: &[&str]) -> Result<Choice> {
        let bin = default_judge_bin();
        let mut command = tokio::process::Command::new(&bin);
        command
            .arg("choice")
            .arg("-i")
            .arg(scrub_for_external(instructions))
            .arg("-q");
        for option in options {
            command.arg("-o").arg(*option);
        }
        for (key, value) in typesafe_env(&self.typesafe) {
            command.env(key, value);
        }
        let output = tokio::time::timeout(Duration::from_secs(5), command.output())
            .await
            .map_err(|_| HubError::message("typesafe-judge timed out"))?
            .map_err(|error| HubError::message(error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() && options.contains(&stdout.as_str()) {
            Ok(Choice::new(stdout))
        } else {
            Err(HubError::message(format!(
                "typesafe-judge declined: {stdout}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typesafe_env_comes_from_config_and_does_not_read_a_shell_file() {
        let mut config = BTreeMap::new();
        config.insert("TYPESAFE_API_KEY".into(), "from-config".into());
        config.insert("UNRELATED".into(), "no".into());
        let env = typesafe_env(&config);
        assert_eq!(env, vec![("TYPESAFE_API_KEY".into(), "from-config".into())]);
        let scrubbed = scrub_for_external("token sk-abcdefghij123456");
        assert!(!scrubbed.contains("sk-abcdefghij123456"));
    }

    #[tokio::test]
    async fn the_offline_fallback_is_marked_as_fallback() {
        let judge = Judge::fallback_yes();
        let choice = judge
            .choice(Purpose::DoneDiff, "evidence", "done?", &["yes", "no"])
            .await
            .unwrap();
        assert!(choice.is_fallback);
        assert_eq!(choice.as_str(), "yes");
    }
}
