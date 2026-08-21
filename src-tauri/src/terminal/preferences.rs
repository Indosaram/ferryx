use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = "monospace";
pub const DEFAULT_MACOS_OPTION_AS_ALT: bool = false;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GhosttyTerminalConfig {
    pub font_family: Option<String>,
    pub macos_option_as_alt: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GhosttyConfigError {
    #[error("malformed Ghostty config line {line}: expected key = value")]
    MalformedLine { line: usize },
    #[error("Ghostty config key '{key}' on line {line} cannot be empty")]
    EmptyValue { line: usize, key: String },
    #[error("Ghostty config key 'macos-option-as-alt' on line {line} must be true, false, left, right, or both")]
    InvalidMacosOptionAsAlt { line: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalPreferencesSource {
    Defaults,
    Ghostty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalPreferencesStatus {
    Imported,
    Absent,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalPreferences {
    pub font_family: String,
    pub macos_option_as_alt: bool,
    pub source: TerminalPreferencesSource,
    pub status: TerminalPreferencesStatus,
    pub source_path: Option<PathBuf>,
}

impl TerminalPreferences {
    fn defaults(status: TerminalPreferencesStatus, source_path: Option<PathBuf>) -> Self {
        Self {
            font_family: DEFAULT_TERMINAL_FONT_FAMILY.to_string(),
            macos_option_as_alt: DEFAULT_MACOS_OPTION_AS_ALT,
            source: TerminalPreferencesSource::Defaults,
            status,
            source_path,
        }
    }

    fn imported(config: GhosttyTerminalConfig, source_path: PathBuf) -> Self {
        Self {
            font_family: config
                .font_family
                .unwrap_or_else(|| DEFAULT_TERMINAL_FONT_FAMILY.to_string()),
            macos_option_as_alt: config
                .macos_option_as_alt
                .unwrap_or(DEFAULT_MACOS_OPTION_AS_ALT),
            source: TerminalPreferencesSource::Ghostty,
            status: TerminalPreferencesStatus::Imported,
            source_path: Some(source_path),
        }
    }
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn format_font_name_for_css(font: &str) -> String {
    let unquoted = unquote(font).trim();
    if unquoted.contains(' ') || unquoted.contains(',') {
        format!("\"{}\"", unquoted)
    } else {
        unquoted.to_string()
    }
}

pub fn parse_ghostty_config(input: &str) -> Result<GhosttyTerminalConfig, GhosttyConfigError> {
    let mut font_families: Vec<String> = Vec::new();
    let mut macos_option_as_alt: Option<bool> = None;

    for (index, raw_line) in input.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            return Err(GhosttyConfigError::MalformedLine { line: line_number });
        };
        let key = raw_key.trim();
        let value = raw_value.trim();

        match key {
            "font-family" => {
                let cleaned = unquote(value).trim();
                if cleaned.is_empty() {
                    return Err(GhosttyConfigError::EmptyValue {
                        line: line_number,
                        key: key.to_string(),
                    });
                }
                font_families.push(cleaned.to_string());
            }
            "macos-option-as-alt" => {
                let cleaned = unquote(value).trim();
                if cleaned.is_empty() {
                    return Err(GhosttyConfigError::EmptyValue {
                        line: line_number,
                        key: key.to_string(),
                    });
                }
                let parsed = if cleaned.eq_ignore_ascii_case("true")
                    || cleaned.eq_ignore_ascii_case("left")
                    || cleaned.eq_ignore_ascii_case("right")
                    || cleaned.eq_ignore_ascii_case("both")
                {
                    true
                } else if cleaned.eq_ignore_ascii_case("false") {
                    false
                } else {
                    return Err(GhosttyConfigError::InvalidMacosOptionAsAlt { line: line_number });
                };
                macos_option_as_alt = Some(parsed);
            }
            _ => {}
        }
    }

    let font_family = if font_families.is_empty() {
        None
    } else {
        let formatted = font_families
            .iter()
            .map(|f| format_font_name_for_css(f))
            .collect::<Vec<_>>()
            .join(", ");
        Some(formatted)
    };

    Ok(GhosttyTerminalConfig {
        font_family,
        macos_option_as_alt,
    })
}

pub fn load_terminal_preferences_from_path(path: &Path) -> TerminalPreferences {
    match fs::read_to_string(path) {
        Ok(contents) => match parse_ghostty_config(&contents) {
            Ok(config) => TerminalPreferences::imported(config, path.to_path_buf()),
            Err(_) => TerminalPreferences::defaults(
                TerminalPreferencesStatus::Malformed,
                Some(path.to_path_buf()),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            TerminalPreferences::defaults(
                TerminalPreferencesStatus::Absent,
                Some(path.to_path_buf()),
            )
        }
        Err(_) => TerminalPreferences::defaults(
            TerminalPreferencesStatus::Malformed,
            Some(path.to_path_buf()),
        ),
    }
}

fn ghostty_config_candidates() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();

    if cfg!(target_os = "macos") {
        if let Some(home) = &home {
            candidates.push(
                home.join("Library")
                    .join("Application Support")
                    .join("com.mitchellh.ghostty")
                    .join("config"),
            );
        }
    }

    if let Some(xdg_config_home) = env::var_os("XDG_CONFIG_HOME") {
        candidates.push(
            PathBuf::from(xdg_config_home)
                .join("ghostty")
                .join("config"),
        );
    }

    if let Some(home) = home {
        candidates.push(home.join(".config").join("ghostty").join("config"));
    }

    candidates.dedup();
    candidates
}

pub fn load_terminal_preferences() -> TerminalPreferences {
    let candidates = ghostty_config_candidates();
    if let Some(path) = candidates.iter().find(|path| path.is_file()) {
        return load_terminal_preferences_from_path(path);
    }

    TerminalPreferences::defaults(
        TerminalPreferencesStatus::Absent,
        candidates.into_iter().next(),
    )
}
