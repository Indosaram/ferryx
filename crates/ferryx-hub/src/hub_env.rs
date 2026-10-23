//! Hub settings that omo-gateway read from its launchd environment.
//!
//! omo's plist carried `OMON_GITHUB_SOURCE_ENABLED`, `OMON_DEFAULT_EFFORT`, `SLACK_HOME_CHANNEL`,
//! `OMON_OMO_BIN` and friends in `EnvironmentVariables`. The ferryx daemon is started with its
//! own environment, so those settings are copied once into `~/.ferryx/hub/env` (mode 0600,
//! `KEY=VALUE` lines) by `ferryx hub env import --from-plist <plist>`. The hub reads every
//! setting through [`var`]: the daemon's own environment wins, then the file. Child processes
//! get the file's entries too, unless the daemon already sets them, as omo's children did.
//! Values are never logged; only key names are reported.

use std::collections::BTreeMap;
use std::env::VarError;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::{HubError, Result};

/// Keys that describe the process rather than a hub setting. They are not imported.
const PROCESS_KEYS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "PWD",
    "OLDPWD",
    "SHLVL",
    "_",
    "__CF_USER_TEXT_ENCODING",
    "XPC_FLAGS",
    "XPC_SERVICE_NAME",
    "SSH_AUTH_SOCK",
];

static FILE_ENV: OnceLock<BTreeMap<String, String>> = OnceLock::new();

/// `~/.ferryx/hub/env`, or `FERRYX_HUB_ENV_FILE`.
pub fn env_file_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FERRYX_HUB_ENV_FILE").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferryx/hub/env"))
}

/// Entries from the hub env file, read once. A missing or unreadable file is empty.
pub fn file_env() -> &'static BTreeMap<String, String> {
    FILE_ENV.get_or_init(|| {
        let Some(path) = env_file_path() else {
            return BTreeMap::new();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let entries = parse_env_file(&text);
                tracing::info!(path = %path.display(), keys = entries.len(), "hub env file loaded");
                entries
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "hub env file was not read");
                BTreeMap::new()
            }
        }
    })
}

/// The daemon environment first, then the hub env file.
pub fn var(key: &str) -> std::result::Result<String, VarError> {
    lookup(key, std::env::var(key), file_env())
}

/// [`var`] as an `OsString`.
pub fn var_os(key: &str) -> Option<OsString> {
    std::env::var_os(key).or_else(|| file_env().get(key).map(OsString::from))
}

fn lookup(
    key: &str,
    process: std::result::Result<String, VarError>,
    file: &BTreeMap<String, String>,
) -> std::result::Result<String, VarError> {
    match process {
        Err(VarError::NotPresent) => file.get(key).cloned().ok_or(VarError::NotPresent),
        other => other,
    }
}

/// File entries the daemon environment does not set, for child processes.
pub fn child_env() -> Vec<(String, String)> {
    child_env_from(file_env(), |key| std::env::var_os(key).is_some())
}

fn child_env_from(
    file: &BTreeMap<String, String>,
    present: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    file.iter()
        .filter(|(key, _)| !present(key) && !PROCESS_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Parses `KEY=VALUE` lines. `export ` prefixes, blank lines and `#` comments are allowed;
/// a value wrapped in matching single or double quotes is unwrapped.
pub fn parse_env_file(text: &str) -> BTreeMap<String, String> {
    let mut entries = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !valid_key(key) {
            continue;
        }
        entries.insert(key.to_owned(), unquote(value.trim()).to_owned());
    }
    entries
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !key.starts_with(|c: char| c.is_ascii_digit())
}

/// One line per entry. Values with newlines cannot round-trip through the file and are skipped.
pub fn render_env_file(entries: &BTreeMap<String, String>) -> String {
    let mut text = String::from(
        "# Hub settings imported from omo-gateway. Mode 0600; holds secrets, do not share.\n",
    );
    for (key, value) in entries {
        if value.contains('\n') || value.contains('\r') {
            continue;
        }
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
    text
}

/// `EnvironmentVariables` of a launchd plist, minus process keys.
pub fn plist_environment(plist: &Path) -> Result<BTreeMap<String, String>> {
    let value = plist::Value::from_file(plist)
        .map_err(|error| HubError::message(format!("{}: {error}", plist.display())))?;
    let Some(env) = value
        .as_dictionary()
        .and_then(|dict| dict.get("EnvironmentVariables"))
        .and_then(plist::Value::as_dictionary)
    else {
        return Ok(BTreeMap::new());
    };
    Ok(env
        .iter()
        .filter(|(key, _)| valid_key(key) && !PROCESS_KEYS.contains(&key.as_str()))
        .filter_map(|(key, value)| {
            value
                .as_string()
                .map(|value| (key.clone(), value.to_owned()))
        })
        .collect())
}

/// Merges `incoming` over the existing file at `dest` and writes it with mode 0600 through a
/// temporary file and rename. Returns the key names written; values are never returned.
pub fn write_merged(dest: &Path, incoming: &BTreeMap<String, String>) -> Result<Vec<String>> {
    let mut entries = match std::fs::read_to_string(dest) {
        Ok(text) => parse_env_file(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(HubError::message(format!("{}: {error}", dest.display()))),
    };
    for (key, value) in incoming {
        entries.insert(key.clone(), value.clone());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|error| HubError::message(error.to_string()))?;
    }
    let staged = dest.with_extension(format!("tmp-{}", std::process::id()));
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&staged)
            .map_err(|error| HubError::message(format!("{}: {error}", staged.display())))?;
        file.write_all(render_env_file(&entries).as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|error| HubError::message(error.to_string()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| HubError::message(error.to_string()))?;
    }
    std::fs::rename(&staged, dest).map_err(|error| HubError::message(error.to_string()))?;
    Ok(entries.keys().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-hub-env-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn env_file_lines_parse_with_exports_quotes_and_comments() {
        let parsed = parse_env_file(
            "# comment\nexport OMON_GITHUB_SOURCE_ENABLED=1\nSLACK_HOME_CHANNEL=\"C1,C2\"\nOMON_DEFAULT_EFFORT='high'\n\nbad line\n9X=no\nOMON_OMO_BIN=/Users/me/.bun/bin/omo\n",
        );
        assert_eq!(parsed.get("OMON_GITHUB_SOURCE_ENABLED").unwrap(), "1");
        assert_eq!(parsed.get("SLACK_HOME_CHANNEL").unwrap(), "C1,C2");
        assert_eq!(parsed.get("OMON_DEFAULT_EFFORT").unwrap(), "high");
        assert_eq!(
            parsed.get("OMON_OMO_BIN").unwrap(),
            "/Users/me/.bun/bin/omo"
        );
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn the_daemon_environment_wins_over_the_file() {
        let mut file = BTreeMap::new();
        file.insert("OMON_DEFAULT_EFFORT".to_owned(), "high".to_owned());
        assert_eq!(
            lookup("OMON_DEFAULT_EFFORT", Err(VarError::NotPresent), &file).unwrap(),
            "high"
        );
        assert_eq!(
            lookup("OMON_DEFAULT_EFFORT", Ok("low".into()), &file).unwrap(),
            "low"
        );
        assert!(lookup("MISSING", Err(VarError::NotPresent), &file).is_err());
    }

    #[test]
    fn children_get_file_entries_the_daemon_does_not_set() {
        let mut file = BTreeMap::new();
        file.insert("OMON_OMO_BIN".to_owned(), "/x/omo".to_owned());
        file.insert("SLACK_HOME_CHANNEL".to_owned(), "C1".to_owned());
        file.insert("PATH".to_owned(), "/nope".to_owned());
        let env = child_env_from(&file, |key| key == "SLACK_HOME_CHANNEL");
        assert_eq!(env, vec![("OMON_OMO_BIN".to_owned(), "/x/omo".to_owned())]);
    }

    #[test]
    fn a_launchd_plist_imports_into_a_private_env_file() {
        let dir = scratch("plist");
        let plist = dir.join("ai.omon.gateway.plist");
        std::fs::write(
            &plist,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>ai.omon.gateway</string>
<key>EnvironmentVariables</key><dict>
<key>PATH</key><string>/opt/homebrew/bin:/usr/bin</string>
<key>HOME</key><string>/Users/me</string>
<key>OMON_GITHUB_SOURCE_ENABLED</key><string>1</string>
<key>OMON_DEFAULT_EFFORT</key><string>high</string>
<key>SLACK_HOME_CHANNEL</key><string>CHOME</string>
<key>OMON_OMO_BIN</key><string>/Users/me/.bun/bin/omo</string>
</dict></dict></plist>"#,
        )
        .unwrap();
        let env = plist_environment(&plist).unwrap();
        assert!(!env.contains_key("PATH") && !env.contains_key("HOME"));
        let dest = dir.join("hub/env");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "KEEP=me\nOMON_DEFAULT_EFFORT=low\n").unwrap();
        let keys = write_merged(&dest, &env).unwrap();
        assert_eq!(
            keys,
            [
                "KEEP",
                "OMON_DEFAULT_EFFORT",
                "OMON_GITHUB_SOURCE_ENABLED",
                "OMON_OMO_BIN",
                "SLACK_HOME_CHANNEL"
            ]
        );
        let written = parse_env_file(&std::fs::read_to_string(&dest).unwrap());
        assert_eq!(written.get("OMON_DEFAULT_EFFORT").unwrap(), "high");
        assert_eq!(written.get("KEEP").unwrap(), "me");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
