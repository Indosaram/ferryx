use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = "monospace";
pub const DEFAULT_MACOS_OPTION_AS_ALT: bool = false;
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = 13.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalThemeColors {
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub cursor_accent: String,
    pub selection_background: String,
    pub selection_foreground: Option<String>,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
    pub extended_ansi: Vec<String>,
}

impl Default for TerminalThemeColors {
    fn default() -> Self {
        Self {
            background: "#282c34".to_string(),
            foreground: "#ffffff".to_string(),
            cursor: "#ffffff".to_string(),
            cursor_accent: "#282c34".to_string(),
            selection_background: "#52525299".to_string(),
            selection_foreground: None,
            black: "#1d1f21".to_string(),
            red: "#cc6666".to_string(),
            green: "#b5bd68".to_string(),
            yellow: "#f0c674".to_string(),
            blue: "#81a2be".to_string(),
            magenta: "#b294bb".to_string(),
            cyan: "#8abeb7".to_string(),
            white: "#c5c8c6".to_string(),
            bright_black: "#666666".to_string(),
            bright_red: "#d54e53".to_string(),
            bright_green: "#b9ca4a".to_string(),
            bright_yellow: "#e7c547".to_string(),
            bright_blue: "#7aa6da".to_string(),
            bright_magenta: "#c397d8".to_string(),
            bright_cyan: "#70c0b1".to_string(),
            bright_white: "#eaeaea".to_string(),
            extended_ansi: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GhosttyTerminalConfig {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub macos_option_as_alt: Option<bool>,
    pub cursor_style: Option<String>,
    pub cursor_color: Option<String>,
    pub cursor_text: Option<String>,
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub selection_background: Option<String>,
    pub selection_foreground: Option<String>,
    pub palette: HashMap<u8, String>,
    pub theme_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GhosttyConfigError {
    #[error("malformed Ghostty config line {line}: expected key = value")]
    MalformedLine { line: usize },
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalPreferences {
    pub font_family: String,
    pub font_size: f32,
    pub macos_option_as_alt: bool,
    pub cursor_style: String,
    pub theme: TerminalThemeColors,
    pub source: TerminalPreferencesSource,
    pub status: TerminalPreferencesStatus,
    pub source_path: Option<PathBuf>,
}

impl TerminalPreferences {
    fn defaults(status: TerminalPreferencesStatus, source_path: Option<PathBuf>) -> Self {
        Self {
            font_family: DEFAULT_TERMINAL_FONT_FAMILY.to_string(),
            font_size: DEFAULT_TERMINAL_FONT_SIZE,
            macos_option_as_alt: DEFAULT_MACOS_OPTION_AS_ALT,
            cursor_style: "block".to_string(),
            theme: TerminalThemeColors::default(),
            source: TerminalPreferencesSource::Defaults,
            status,
            source_path,
        }
    }

    fn imported(config: GhosttyTerminalConfig, source_path: PathBuf) -> Self {
        let mut theme = TerminalThemeColors::default();
        if let Some(bg) = config.background {
            theme.background = bg;
        }
        if let Some(fg) = config.foreground {
            theme.foreground = fg;
        }
        if let Some(cursor) = config.cursor_color {
            theme.cursor = cursor;
        }
        if let Some(cursor_text) = config.cursor_text {
            theme.cursor_accent = cursor_text;
        }
        if let Some(sel) = config.selection_background {
            theme.selection_background = sel;
        }
        if let Some(sel_fg) = config.selection_foreground {
            theme.selection_foreground = Some(sel_fg);
        }

        let palette_map: [(&str, &mut String); 16] = [
            ("0", &mut theme.black),
            ("1", &mut theme.red),
            ("2", &mut theme.green),
            ("3", &mut theme.yellow),
            ("4", &mut theme.blue),
            ("5", &mut theme.magenta),
            ("6", &mut theme.cyan),
            ("7", &mut theme.white),
            ("8", &mut theme.bright_black),
            ("9", &mut theme.bright_red),
            ("10", &mut theme.bright_green),
            ("11", &mut theme.bright_yellow),
            ("12", &mut theme.bright_blue),
            ("13", &mut theme.bright_magenta),
            ("14", &mut theme.bright_cyan),
            ("15", &mut theme.bright_white),
        ];

        for (idx_str, target) in palette_map {
            if let Ok(idx) = idx_str.parse::<u8>() {
                if let Some(color) = config.palette.get(&idx) {
                    *target = color.clone();
                }
            }
        }

        let mut extended = Vec::new();
        for idx in 16u8..=255u8 {
            if let Some(color) = config.palette.get(&idx) {
                extended.push(color.clone());
            }
        }
        theme.extended_ansi = extended;

        let cursor_style = match config.cursor_style.as_deref() {
            Some("bar") => "bar".to_string(),
            Some("underline") => "underline".to_string(),
            _ => "block".to_string(),
        };

        Self {
            font_family: config
                .font_family
                .unwrap_or_else(|| DEFAULT_TERMINAL_FONT_FAMILY.to_string()),
            font_size: config.font_size.unwrap_or(DEFAULT_TERMINAL_FONT_SIZE),
            macos_option_as_alt: config
                .macos_option_as_alt
                .unwrap_or(DEFAULT_MACOS_OPTION_AS_ALT),
            cursor_style,
            theme,
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

fn normalize_color(color: &str) -> String {
    let trimmed = unquote(color).trim();
    if (trimmed.len() == 6 || trimmed.len() == 8) && trimmed.chars().all(|c| c.is_ascii_hexdigit())
    {
        format!("#{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

pub fn parse_ghostty_config(input: &str) -> Result<GhosttyTerminalConfig, GhosttyConfigError> {
    let mut font_families: Vec<String> = Vec::new();
    let mut font_size: Option<f32> = None;
    let mut macos_option_as_alt: Option<bool> = None;
    let mut cursor_style: Option<String> = None;
    let mut cursor_color: Option<String> = None;
    let mut cursor_text: Option<String> = None;
    let mut background: Option<String> = None;
    let mut foreground: Option<String> = None;
    let mut selection_background: Option<String> = None;
    let mut selection_foreground: Option<String> = None;
    let mut palette: HashMap<u8, String> = HashMap::new();
    let mut theme_name: Option<String> = None;

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
                    font_families.clear();
                } else if !font_families.contains(&cleaned.to_string()) {
                    font_families.push(cleaned.to_string());
                }
            }
            "font-size" => {
                let cleaned = unquote(value).trim();
                if cleaned.is_empty() {
                    font_size = None;
                } else if let Ok(size) = cleaned.parse::<f32>() {
                    if size > 0.0 {
                        font_size = Some(size);
                    }
                }
            }
            "macos-option-as-alt" => {
                let cleaned = unquote(value).trim();
                if cleaned.is_empty() {
                    macos_option_as_alt = None;
                } else {
                    let parsed = if cleaned.eq_ignore_ascii_case("true")
                        || cleaned.eq_ignore_ascii_case("left")
                        || cleaned.eq_ignore_ascii_case("right")
                        || cleaned.eq_ignore_ascii_case("both")
                    {
                        true
                    } else if cleaned.eq_ignore_ascii_case("false") {
                        false
                    } else {
                        return Err(GhosttyConfigError::InvalidMacosOptionAsAlt {
                            line: line_number,
                        });
                    };
                    macos_option_as_alt = Some(parsed);
                }
            }
            "cursor-style" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    cursor_style = Some(cleaned.to_string());
                }
            }
            "cursor-color" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    cursor_color = Some(normalize_color(cleaned));
                }
            }
            "cursor-text" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    cursor_text = Some(normalize_color(cleaned));
                }
            }
            "background" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    background = Some(normalize_color(cleaned));
                }
            }
            "foreground" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    foreground = Some(normalize_color(cleaned));
                }
            }
            "selection-background" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    selection_background = Some(normalize_color(cleaned));
                }
            }
            "selection-foreground" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    selection_foreground = Some(normalize_color(cleaned));
                }
            }
            "theme" => {
                let cleaned = unquote(value).trim();
                if !cleaned.is_empty() {
                    theme_name = Some(cleaned.to_string());
                }
            }
            "palette" => {
                let cleaned = unquote(value).trim();
                if let Some((idx_str, color_str)) = cleaned.split_once('=') {
                    let trimmed_idx = idx_str.trim();
                    let parsed_idx = if let Some(hex) = trimmed_idx
                        .strip_prefix("0x")
                        .or_else(|| trimmed_idx.strip_prefix("0X"))
                    {
                        u8::from_str_radix(hex, 16).ok()
                    } else {
                        trimmed_idx.parse::<u8>().ok()
                    };
                    if let Some(idx) = parsed_idx {
                        palette.insert(idx, normalize_color(color_str.trim()));
                    }
                }
            }
            _ => {}
        }
    }

    let font_family = if font_families.is_empty() {
        None
    } else {
        let mut parts: Vec<String> = font_families.clone();
        if !parts.iter().any(|p| p.eq_ignore_ascii_case("monospace")) {
            parts.push("monospace".to_string());
        }
        Some(parts.join(", "))
    };

    Ok(GhosttyTerminalConfig {
        font_family,
        font_size,
        macos_option_as_alt,
        cursor_style,
        cursor_color,
        cursor_text,
        background,
        foreground,
        selection_background,
        selection_foreground,
        palette,
        theme_name,
    })
}

fn try_load_from_ghostty_cli() -> Option<GhosttyTerminalConfig> {
    let mut cmd = Command::new("ghostty");
    cmd.arg("+show-config");

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_ghostty_config(&text).ok()
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
                    .join("config.ghostty"),
            );
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
            PathBuf::from(&xdg_config_home)
                .join("ghostty")
                .join("config.ghostty"),
        );
        candidates.push(
            PathBuf::from(xdg_config_home)
                .join("ghostty")
                .join("config"),
        );
    }

    if let Some(home) = home {
        candidates.push(home.join(".config").join("ghostty").join("config.ghostty"));
        candidates.push(home.join(".config").join("ghostty").join("config"));
    }

    candidates.dedup();
    candidates
}

pub fn load_terminal_preferences() -> TerminalPreferences {
    if let Some(cli_config) = try_load_from_ghostty_cli() {
        return TerminalPreferences::imported(cli_config, PathBuf::from("ghostty"));
    }

    let candidates = ghostty_config_candidates();
    if let Some(path) = candidates.iter().find(|path| path.is_file()) {
        return load_terminal_preferences_from_path(path);
    }

    TerminalPreferences::defaults(
        TerminalPreferencesStatus::Absent,
        candidates.into_iter().next(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_font_family_resets_list() {
        let config_str = "font-family = \"MesloLGS NF\"\nfont-family =\nfont-family = \"Fira Code\"";
        let parsed = parse_ghostty_config(config_str).unwrap();
        assert_eq!(parsed.font_family.as_deref(), Some("Fira Code, monospace"));
    }

    #[test]
    fn test_palette_hex_and_256_colors() {
        let config_str = "palette = 0x01=#ff0000\npalette = 16=#0000ff\npalette = 255=#ffffff";
        let parsed = parse_ghostty_config(config_str).unwrap();
        assert_eq!(parsed.palette.get(&1), Some(&"#ff0000".to_string()));
        assert_eq!(parsed.palette.get(&16), Some(&"#0000ff".to_string()));
        assert_eq!(parsed.palette.get(&255), Some(&"#ffffff".to_string()));

        let mut prefs_config = GhosttyTerminalConfig::default();
        for idx in 16u8..=255u8 {
            prefs_config.palette.insert(idx, format!("#color_{}", idx));
        }
        let prefs = TerminalPreferences::imported(prefs_config, PathBuf::from("test"));
        assert_eq!(prefs.theme.extended_ansi.len(), 240);
        assert_eq!(prefs.theme.extended_ansi[0], "#color_16");
    }
}
