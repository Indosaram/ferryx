use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub const SECRETS_REL_PATH: &str = ".config/compute-secrets.env";
pub const SECRETS_FILE_ENV: &str = "FERRYX_CLOUD_SECRETS_FILE";

pub const ALLOWED_KEYS: &[&str] = &[
    "DAYTONA_API_KEY",
    "E2B_API_KEY",
    "FREESTYLE_API_KEY",
    "BL_API_KEY",
    "BL_WORKSPACE",
    "FERRYX_CODESPACE_REPO",
    "FERRYX_CODESPACE_MACHINE",
];

pub const AUTO_PROVIDER_ORDER: &[&str] = &[
    "daytona",
    "e2b",
    "freestyle",
    "blaxel",
    "codespaces",
    "instabox",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub transport: &'static str,
    pub free_quota: &'static str,
    pub configured: bool,
    pub secret_keys: Vec<&'static str>,
    pub missing_keys: Vec<&'static str>,
    pub note: &'static str,
}

pub fn secrets_file_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var(SECRETS_FILE_ENV) {
        let trimmed = explicit.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(PathBuf::from(trimmed));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(SECRETS_REL_PATH))
}

pub fn load_var(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let path = secrets_file_path()?;
    load_var_from(&path, key)
}

pub fn load_var_from(path: &Path, key: &str) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    parse_env_map(&contents).remove(key)
}

pub fn parse_env_map(contents: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let stripped = line.strip_prefix("export ").map(str::trim_start).unwrap_or(line);
        let Some((k, v)) = stripped.split_once('=') else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let val = unquote_env_value(v.trim());
        if !val.is_empty() {
            out.insert(key.to_string(), val);
        }
    }
    out
}

fn unquote_env_value(raw: &str) -> String {
    if raw.len() >= 2
        && ((raw.starts_with('"') && raw.ends_with('"'))
            || (raw.starts_with('\'') && raw.ends_with('\'')))
    {
        raw[1..raw.len() - 1].to_string()
    } else {
        raw.to_string()
    }
}

pub fn save_var(key: &str, value: Option<&str>) -> Result<(), String> {
    let path = secrets_file_path()
        .ok_or_else(|| "cannot resolve ~/.config/compute-secrets.env path".to_string())?;
    save_var_in(&path, key, value)
}

pub fn save_var_in(path: &Path, key: &str, value: Option<&str>) -> Result<(), String> {
    if !ALLOWED_KEYS.contains(&key) {
        return Err(format!("unsupported cloud secret key `{key}`"));
    }
    let clean_value = match value.map(str::trim) {
        Some("") | None => None,
        Some(v) => {
            if v.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
                return Err(format!("value for `{key}` must be a single line without control characters"));
            }
            Some(v)
        }
    };

    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };

    let mut lines: Vec<String> = Vec::new();
    let mut replaced = false;
    for raw in existing.lines() {
        let trimmed = raw.trim();
        let entry = trimmed.strip_prefix("export ").map(str::trim_start).unwrap_or(trimmed);
        let matches_key = entry
            .split_once('=')
            .is_some_and(|(k, _)| k.trim() == key);
        if matches_key {
            if let Some(val) = clean_value {
                if !replaced {
                    lines.push(format_env_line(key, val));
                    replaced = true;
                }
            }
        } else {
            lines.push(raw.to_string());
        }
    }
    if let Some(val) = clean_value {
        if !replaced {
            lines.push(format_env_line(key, val));
        }
    }

    let mut rendered = lines.join("\n");
    if !rendered.is_empty() {
        rendered.push('\n');
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("compute-secrets.env");
    let tmp = path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()));
    write_private_file(&tmp, rendered.as_bytes())?;
    std::fs::rename(&tmp, path).map_err(|error| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace {}: {error}", path.display())
    })?;
    Ok(())
}

fn format_env_line(key: &str, value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("export {key}='{escaped}'")
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn provider_statuses() -> Vec<ProviderStatus> {
    let check_keys = |keys: &[&'static str]| -> Vec<&'static str> {
        keys.iter()
            .copied()
            .filter(|k| load_var(k).is_none())
            .collect()
    };

    let daytona_missing = check_keys(&["DAYTONA_API_KEY"]);
    let e2b_missing = check_keys(&["E2B_API_KEY"]);
    let freestyle_missing = check_keys(&["FREESTYLE_API_KEY"]);
    let blaxel_missing = check_keys(&["BL_API_KEY", "BL_WORKSPACE"]);
    let codespaces_missing = check_keys(&["FERRYX_CODESPACE_REPO"]);
    let gh_present = command_on_path("gh");
    let instabox_present = command_on_path("instabox");

    vec![
        ProviderStatus {
            id: "daytona",
            label: "Daytona",
            transport: "ssh",
            free_quota: "$200 credit (no card)",
            configured: daytona_missing.is_empty(),
            secret_keys: vec!["DAYTONA_API_KEY"],
            missing_keys: daytona_missing,
            note: "Default auto provider · SSH + model bridge",
        },
        ProviderStatus {
            id: "e2b",
            label: "E2B",
            transport: "relay",
            free_quota: "$100 credit · 1h max/box",
            configured: e2b_missing.is_empty(),
            secret_keys: vec!["E2B_API_KEY"],
            missing_keys: e2b_missing,
            note: "Fallback auto provider · automatic relay enrollment",
        },
        ProviderStatus {
            id: "freestyle",
            label: "Freestyle",
            transport: "ssh",
            free_quota: "200 vCPU-h / mo",
            configured: freestyle_missing.is_empty(),
            secret_keys: vec!["FREESTYLE_API_KEY"],
            missing_keys: freestyle_missing,
            note: "Monthly recurring free tier · SSH",
        },
        ProviderStatus {
            id: "blaxel",
            label: "Blaxel",
            transport: "relay",
            free_quota: "Up to $200 credit",
            configured: blaxel_missing.is_empty(),
            secret_keys: vec!["BL_API_KEY", "BL_WORKSPACE"],
            missing_keys: blaxel_missing,
            note: "Requires BL_API_KEY and BL_WORKSPACE",
        },
        ProviderStatus {
            id: "codespaces",
            label: "GitHub Codespaces",
            transport: "ssh",
            free_quota: "120 core-h / mo",
            configured: gh_present && codespaces_missing.is_empty(),
            secret_keys: vec!["FERRYX_CODESPACE_REPO"],
            missing_keys: codespaces_missing,
            note: if gh_present {
                "Uses signed-in `gh` CLI + target repository (owner/repo)"
            } else {
                "Install & sign in with `gh` CLI first"
            },
        },
        ProviderStatus {
            id: "instabox",
            label: "InstaBox",
            transport: "ssh",
            free_quota: "1 always-on slot (4 vCPU / 8 GB)",
            configured: instabox_present,
            secret_keys: vec![],
            missing_keys: vec![],
            note: if instabox_present {
                "Uses `instabox` CLI session (1-slot limit)"
            } else {
                "Install & sign in with `instabox` CLI"
            },
        },
    ]
}

pub fn configured_providers_in_order() -> Vec<&'static str> {
    let statuses = provider_statuses();
    AUTO_PROVIDER_ORDER
        .iter()
        .copied()
        .filter(|id| statuses.iter().any(|s| s.id == *id && s.configured))
        .collect()
}

fn command_on_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_map_handles_export_quotes_and_comments() {
        let text = "\
# comment
export DAYTONA_API_KEY=\"dtn_test_123\"
E2B_API_KEY='e2b_test_456'
FREESTYLE_API_KEY=fs_plain
EMPTY_KEY=
not a valid line
";
        let map = parse_env_map(text);
        assert_eq!(map.get("DAYTONA_API_KEY").map(String::as_str), Some("dtn_test_123"));
        assert_eq!(map.get("E2B_API_KEY").map(String::as_str), Some("e2b_test_456"));
        assert_eq!(map.get("FREESTYLE_API_KEY").map(String::as_str), Some("fs_plain"));
        assert!(!map.contains_key("EMPTY_KEY"));
    }

    #[test]
    fn save_var_in_creates_updates_preserves_siblings_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compute-secrets.env");

        save_var_in(&path, "DAYTONA_API_KEY", Some("val-1")).unwrap();
        save_var_in(&path, "E2B_API_KEY", Some("val-2")).unwrap();
        assert_eq!(load_var_from(&path, "DAYTONA_API_KEY").as_deref(), Some("val-1"));
        assert_eq!(load_var_from(&path, "E2B_API_KEY").as_deref(), Some("val-2"));

        save_var_in(&path, "DAYTONA_API_KEY", Some("val-1-new")).unwrap();
        assert_eq!(load_var_from(&path, "DAYTONA_API_KEY").as_deref(), Some("val-1-new"));
        assert_eq!(load_var_from(&path, "E2B_API_KEY").as_deref(), Some("val-2"));

        save_var_in(&path, "DAYTONA_API_KEY", None).unwrap();
        assert_eq!(load_var_from(&path, "DAYTONA_API_KEY"), None);
        assert_eq!(load_var_from(&path, "E2B_API_KEY").as_deref(), Some("val-2"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn save_var_in_rejects_unlisted_keys_and_multiline_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compute-secrets.env");

        assert!(save_var_in(&path, "PATH", Some("/evil")).is_err());
        assert!(save_var_in(&path, "DAYTONA_API_KEY", Some("a\nb")).is_err());
        assert!(!path.exists());
    }
}
