use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub const DEFAULT_TERMINAL_FONT_FAMILY: &str = "monospace";
pub const DEFAULT_MACOS_OPTION_AS_ALT: bool = false;
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = 13.0;
pub const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

fn default_scrollback() -> usize {
    DEFAULT_SCROLLBACK_LINES
}

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette_overrides: Vec<(u8, String)>,
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
            palette_overrides: Vec::new(),
        }
    }
}

impl TerminalThemeColors {
    /// The built-in light palette, used under a light appearance when the Ghostty import
    /// declares no theme and no background of its own.
    pub fn light() -> Self {
        Self {
            background: "#fafafa".to_string(),
            foreground: "#383a42".to_string(),
            cursor: "#526eff".to_string(),
            cursor_accent: "#fafafa".to_string(),
            selection_background: "#bfceff99".to_string(),
            selection_foreground: None,
            black: "#383a42".to_string(),
            red: "#e45649".to_string(),
            green: "#50a14f".to_string(),
            yellow: "#c18401".to_string(),
            blue: "#4078f2".to_string(),
            magenta: "#a626a4".to_string(),
            cyan: "#0184bc".to_string(),
            white: "#a0a1a7".to_string(),
            bright_black: "#696c77".to_string(),
            bright_red: "#ca1243".to_string(),
            bright_green: "#3e953a".to_string(),
            bright_yellow: "#986801".to_string(),
            bright_blue: "#2f5af3".to_string(),
            bright_magenta: "#950095".to_string(),
            bright_cyan: "#0997b3".to_string(),
            bright_white: "#fafafa".to_string(),
            extended_ansi: Vec::new(),
            palette_overrides: Vec::new(),
        }
    }

    /// Default colors for `appearance`.
    pub fn for_appearance(appearance: TerminalAppearance) -> Self {
        match appearance {
            TerminalAppearance::Light => Self::light(),
            TerminalAppearance::Dark => Self::default(),
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
    pub scrollback_limit: Option<usize>,
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
    #[serde(default)]
    pub default_shell: Option<String>,
    #[serde(default = "default_scrollback")]
    pub scrollback: usize,
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
            default_shell: None,
            scrollback: DEFAULT_SCROLLBACK_LINES,
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

        let mut palette_overrides: Vec<(u8, String)> = config.palette.into_iter().collect();
        palette_overrides.sort_by_key(|(idx, _)| *idx);
        theme.palette_overrides = palette_overrides;

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
            default_shell: None,
            scrollback: config.scrollback_limit.unwrap_or(DEFAULT_SCROLLBACK_LINES),
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
    let mut scrollback_limit: Option<usize> = None;

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
            "scrollback-limit" | "scrollback-limit-lines" => {
                let cleaned = unquote(value).trim();
                if cleaned == "unlimited" {
                    scrollback_limit = Some(100_000);
                } else if let Ok(parsed) = cleaned.parse::<usize>() {
                    scrollback_limit = Some(parsed.clamp(1_000, 500_000));
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
        scrollback_limit,
    })
}

/// Resolved app appearance the terminal theme follows.
///
/// Dark is the default so a process that never received an appearance keeps the historical
/// dark-variant behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalAppearance {
    Light,
    #[default]
    Dark,
}

/// Selects the theme name that applies to the terminal surface for `appearance`.
///
/// Ghostty supports `light:<name>,dark:<name>`: the side matching the resolved app appearance
/// wins. When only one side is declared that side applies to both appearances (no light palette
/// is invented), and a plain theme name is returned unchanged.
fn theme_name_for_appearance(raw: &str, appearance: TerminalAppearance) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut light = None;
    let mut dark = None;
    for part in trimmed.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("dark:") {
            dark = Some(rest.trim().to_string());
        } else if let Some(rest) = part.strip_prefix("light:") {
            light = Some(rest.trim().to_string());
        }
    }
    let (preferred, fallback) = match appearance {
        TerminalAppearance::Light => (light, dark),
        TerminalAppearance::Dark => (dark, light),
    };
    preferred
        .or(fallback)
        .or_else(|| Some(trimmed.to_string()))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_ghostty_binary_on_path() -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(if cfg!(windows) {
            "ghostty.exe"
        } else {
            "ghostty"
        });
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Environment-derived directories where Ghostty may keep its configuration.
///
/// Windows normally defines neither `HOME` nor `XDG_CONFIG_HOME`; Ghostty stores its config under
/// `%LOCALAPPDATA%\ghostty` there, with `%APPDATA%` as the secondary convention. Factored into a
/// value type so the candidate roots stay unit-testable without mutating the process environment.
struct GhosttyEnvRoots {
    home: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    app_data: Option<PathBuf>,
}

impl GhosttyEnvRoots {
    fn from_env() -> Self {
        Self {
            // Windows uses `USERPROFILE` instead of `HOME`.
            home: env::var_os("HOME")
                .or_else(|| env::var_os("USERPROFILE"))
                .map(PathBuf::from),
            xdg_config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            local_app_data: env::var_os("LOCALAPPDATA").map(PathBuf::from),
            app_data: env::var_os("APPDATA").map(PathBuf::from),
        }
    }

    /// Ghostty config directories, in lookup order.
    ///
    /// The list is empty only when the process carries no home directory, no `XDG_CONFIG_HOME`,
    /// and neither `LOCALAPPDATA` nor `APPDATA`. That is an explicit "no known config location"
    /// outcome, not a missing default: the caller then reports the import as absent with no
    /// source path instead of probing an invented location.
    fn config_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(xdg_config_home) = &self.xdg_config_home {
            dirs.push(xdg_config_home.join("ghostty"));
        }
        if let Some(home) = &self.home {
            dirs.push(home.join(".config").join("ghostty"));
        }
        if let Some(local_app_data) = &self.local_app_data {
            dirs.push(local_app_data.join("ghostty"));
        }
        if let Some(app_data) = &self.app_data {
            dirs.push(app_data.join("ghostty"));
        }
        dirs
    }
}

/// Config file names searched inside each Ghostty config directory.
const GHOSTTY_CONFIG_FILE_NAMES: [&str; 2] = ["config.ghostty", "config"];

fn config_candidates_from_roots(roots: &GhosttyEnvRoots) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for dir in roots.config_dirs() {
        for file_name in GHOSTTY_CONFIG_FILE_NAMES {
            candidates.push(dir.join(file_name));
        }
    }
    candidates
}

fn theme_candidates_from_roots(roots: &GhosttyEnvRoots, name: &str) -> Vec<PathBuf> {
    roots
        .config_dirs()
        .into_iter()
        .map(|dir| dir.join("themes").join(name))
        .collect()
}

/// Ghostty theme lookup order: user theme dir, then the shipped resources theme dir.
/// A theme name containing path separators is only honored when absolute.
///
/// An empty list is an explicit "nothing to inspect" outcome: the theme name is unusable, or the
/// process exposes no config root and no shipped theme directory was found. Callers then leave the
/// config's own colors in place rather than falling back to a guessed path.
fn theme_config_candidates(name: &str, appearance: TerminalAppearance) -> Vec<PathBuf> {
    let Some(name) = theme_name_for_appearance(name, appearance) else {
        return Vec::new();
    };
    let path = Path::new(&name);
    if path.is_absolute() {
        return vec![path.to_path_buf()];
    }
    if name.contains(std::path::MAIN_SEPARATOR) {
        return Vec::new();
    }

    let mut candidates = theme_candidates_from_roots(&GhosttyEnvRoots::from_env(), &name);
    if let Some(ghostty_bin) = find_ghostty_binary_on_path() {
        if let Some(bin_dir) = ghostty_bin.parent() {
            candidates.push(
                bin_dir
                    .join("..")
                    .join("ghostty")
                    .join("themes")
                    .join(&name),
            );
            candidates.push(
                bin_dir
                    .join("..")
                    .join("share")
                    .join("ghostty")
                    .join("themes")
                    .join(&name),
            );
        }
    }
    if let Some(resources_dir) = env::var_os("GHOSTTY_RESOURCES_DIR") {
        candidates.push(PathBuf::from(resources_dir).join("themes").join(&name));
    }
    if cfg!(target_os = "macos") {
        candidates.push(
            PathBuf::from("/Applications/Ghostty.app/Contents/Resources/ghostty/themes")
                .join(&name),
        );
    }
    candidates
}

fn load_theme_config(name: &str, appearance: TerminalAppearance) -> Option<GhosttyTerminalConfig> {
    for candidate in theme_config_candidates(name, appearance) {
        let Ok(contents) = fs::read_to_string(&candidate) else {
            continue;
        };
        if let Ok(theme) = parse_ghostty_config(&contents) {
            return Some(theme);
        }
    }
    None
}

/// Applies theme colors underneath the config's own colors.
///
/// Ghostty semantics: a theme supplies the color baseline and any explicit `background`,
/// `foreground`, `palette`, ... entry in the config overrides it. Non-color options declared by a
/// theme file are ignored so a theme can never change font metrics.
pub(crate) fn merge_theme_defaults(
    config: &mut GhosttyTerminalConfig,
    theme: GhosttyTerminalConfig,
) {
    for (target, value) in [
        (&mut config.background, theme.background),
        (&mut config.foreground, theme.foreground),
        (&mut config.cursor_color, theme.cursor_color),
        (&mut config.cursor_text, theme.cursor_text),
        (&mut config.selection_background, theme.selection_background),
        (&mut config.selection_foreground, theme.selection_foreground),
    ] {
        if target.is_none() {
            *target = value;
        }
    }

    for (index, color) in theme.palette {
        config.palette.entry(index).or_insert(color);
    }
}

fn resolve_theme(
    mut config: GhosttyTerminalConfig,
    appearance: TerminalAppearance,
) -> GhosttyTerminalConfig {
    if let Some(name) = config.theme_name.clone() {
        if let Some(theme) = load_theme_config(&name, appearance) {
            merge_theme_defaults(&mut config, theme);
        }
    }
    config
}

fn try_load_from_ghostty_cli(appearance: TerminalAppearance) -> Option<GhosttyTerminalConfig> {
    let mut cmd = crate::util::no_window_command("ghostty");
    cmd.arg("+show-config");

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_ghostty_config(&text)
        .ok()
        .map(|config| resolve_theme(config, appearance))
}

pub fn load_terminal_preferences_from_path(path: &Path) -> TerminalPreferences {
    load_terminal_preferences_from_path_for(path, TerminalAppearance::default())
}

/// Loads a Ghostty config file, resolving a `light:`/`dark:` theme for `appearance`.
pub fn load_terminal_preferences_from_path_for(
    path: &Path,
    appearance: TerminalAppearance,
) -> TerminalPreferences {
    match fs::read_to_string(path) {
        Ok(contents) => match parse_ghostty_config(&contents) {
            Ok(config) => imported_for(resolve_theme(config, appearance), path.to_path_buf(), appearance),
            Err(_) => defaults_for(
                TerminalPreferencesStatus::Malformed,
                Some(path.to_path_buf()),
                appearance,
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            defaults_for(TerminalPreferencesStatus::Absent, Some(path.to_path_buf()), appearance)
        }
        Err(_) => defaults_for(
            TerminalPreferencesStatus::Malformed,
            Some(path.to_path_buf()),
            appearance,
        ),
    }
}

/// Defaults with the palette for `appearance`.
fn defaults_for(
    status: TerminalPreferencesStatus,
    source_path: Option<PathBuf>,
    appearance: TerminalAppearance,
) -> TerminalPreferences {
    let mut preferences = TerminalPreferences::defaults(status, source_path);
    preferences.theme = TerminalThemeColors::for_appearance(appearance);
    preferences
}

/// An import that declares no theme and no colors of its own takes the built-in palette for
/// `appearance`; any declared theme or color keeps the import exactly as written.
fn imported_for(
    config: GhosttyTerminalConfig,
    source_path: PathBuf,
    appearance: TerminalAppearance,
) -> TerminalPreferences {
    let declares_colors = config.theme_name.is_some()
        || config.background.is_some()
        || config.foreground.is_some()
        || config.cursor_color.is_some()
        || config.selection_background.is_some()
        || !config.palette.is_empty();
    let mut preferences = TerminalPreferences::imported(config, source_path);
    if !declares_colors {
        preferences.theme = TerminalThemeColors::for_appearance(appearance);
    }
    preferences
}

fn ghostty_config_candidates() -> Vec<PathBuf> {
    let roots = GhosttyEnvRoots::from_env();
    let mut candidates = Vec::new();

    if cfg!(target_os = "macos") {
        if let Some(home) = &roots.home {
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

    // Empty only when no config root exists on this platform at all; the caller then reports the
    // import as absent with no source path instead of guessing a location.
    candidates.extend(config_candidates_from_roots(&roots));

    candidates.dedup();
    candidates
}

pub fn load_terminal_preferences() -> TerminalPreferences {
    load_terminal_preferences_for(TerminalAppearance::default())
}

/// Imports the Ghostty configuration, resolving a `light:`/`dark:` theme for `appearance`.
pub fn load_terminal_preferences_for(appearance: TerminalAppearance) -> TerminalPreferences {
    if let Some(cli_config) = try_load_from_ghostty_cli(appearance) {
        return imported_for(cli_config, PathBuf::from("ghostty"), appearance);
    }

    // An empty candidate list means no config root is known on this platform: the import is
    // reported absent with no source path. Otherwise the first candidate is surfaced as the
    // expected location even when the file does not exist yet.
    let candidates = ghostty_config_candidates();
    if let Some(path) = candidates.iter().find(|path| path.is_file()) {
        return load_terminal_preferences_from_path_for(path, appearance);
    }

    defaults_for(TerminalPreferencesStatus::Absent, candidates.into_iter().next(), appearance)
}

/// Local, user-set terminal preferences that take precedence over the Ghostty import.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerminalPreferenceOverrides {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub macos_option_as_alt: Option<bool>,
    pub shell: Option<String>,
    pub scrollback: Option<usize>,
}

struct PreferenceCache {
    imported: Arc<TerminalPreferences>,
    overrides: TerminalPreferenceOverrides,
    appearance: TerminalAppearance,
    effective: Arc<TerminalPreferences>,
}

static PREFERENCE_CACHE: Mutex<Option<PreferenceCache>> = Mutex::new(None);

pub fn apply_terminal_preference_overrides(
    imported: &TerminalPreferences,
    overrides: &TerminalPreferenceOverrides,
) -> TerminalPreferences {
    let mut effective = imported.clone();
    if let Some(font_family) = overrides
        .font_family
        .as_deref()
        .map(str::trim)
        .filter(|family| !family.is_empty())
    {
        effective.font_family = font_family.to_string();
    }
    if let Some(font_size) = overrides
        .font_size
        .filter(|size| size.is_finite() && *size > 0.0 && *size <= 200.0)
    {
        effective.font_size = font_size;
    }
    if let Some(option_as_alt) = overrides.macos_option_as_alt {
        effective.macos_option_as_alt = option_as_alt;
    }
    if let Some(shell) = &overrides.shell {
        let trimmed = shell.trim();
        if trimmed.is_empty() {
            effective.default_shell = None;
        } else {
            effective.default_shell = Some(trimmed.to_string());
        }
    }
    if let Some(scrollback) = overrides
        .scrollback
        .filter(|lines| *lines >= 1_000 && *lines <= 500_000)
    {
        effective.scrollback = scrollback;
    }
    effective
}

fn rebuild_cache(
    imported: Arc<TerminalPreferences>,
    overrides: TerminalPreferenceOverrides,
    appearance: TerminalAppearance,
) -> PreferenceCache {
    let effective = Arc::new(apply_terminal_preference_overrides(&imported, &overrides));
    PreferenceCache {
        imported,
        overrides,
        appearance,
        effective,
    }
}

fn cache_guard() -> std::sync::MutexGuard<'static, Option<PreferenceCache>> {
    PREFERENCE_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Effective terminal preferences: the Ghostty import with local overrides applied.
///
/// Cached because the import shells out to `ghostty +show-config`, while render and input paths
/// read it per frame and per keystroke.
pub fn cached_terminal_preferences() -> Arc<TerminalPreferences> {
    let mut guard = cache_guard();
    let cache = guard.get_or_insert_with(|| {
        rebuild_cache(
            Arc::new(load_terminal_preferences()),
            TerminalPreferenceOverrides::default(),
            TerminalAppearance::default(),
        )
    });
    Arc::clone(&cache.effective)
}

/// Cached Ghostty import without local overrides applied.
pub fn cached_imported_terminal_preferences() -> Arc<TerminalPreferences> {
    let mut guard = cache_guard();
    let cache = guard.get_or_insert_with(|| {
        rebuild_cache(
            Arc::new(load_terminal_preferences()),
            TerminalPreferenceOverrides::default(),
            TerminalAppearance::default(),
        )
    });
    Arc::clone(&cache.imported)
}

/// Re-reads the Ghostty configuration, preserving the active local overrides and appearance.
///
/// Returns the freshly imported preferences so callers can surface the on-disk state.
pub fn reload_terminal_preferences() -> Arc<TerminalPreferences> {
    let appearance = terminal_appearance();
    let imported = Arc::new(load_terminal_preferences_for(appearance));
    let mut guard = cache_guard();
    let (overrides, appearance) = guard
        .as_ref()
        .map(|cache| (cache.overrides.clone(), cache.appearance))
        .unwrap_or((TerminalPreferenceOverrides::default(), appearance));
    let cache = guard.insert(rebuild_cache(imported, overrides, appearance));
    Arc::clone(&cache.imported)
}

/// Replaces the local overrides layered on top of the Ghostty import.
pub fn set_terminal_preference_overrides(
    overrides: TerminalPreferenceOverrides,
) -> Arc<TerminalPreferences> {
    let mut guard = cache_guard();
    let (imported, appearance) = match guard.take() {
        Some(cache) => (cache.imported, cache.appearance),
        None => (
            Arc::new(load_terminal_preferences()),
            TerminalAppearance::default(),
        ),
    };
    let cache = guard.insert(rebuild_cache(imported, overrides, appearance));
    Arc::clone(&cache.effective)
}

/// Appearance the cached terminal theme currently resolves for (Dark until one is pushed).
pub fn terminal_appearance() -> TerminalAppearance {
    cache_guard()
        .as_ref()
        .map(|cache| cache.appearance)
        .unwrap_or_default()
}

/// Records the resolved app appearance and re-imports the theme for it.
///
/// Returns true only when the effective appearance changed; an unchanged appearance leaves the
/// cache untouched. Reads the Ghostty config from disk on change, so callers on async runtimes
/// must run it through `crate::ipc::run_blocking`.
pub fn set_terminal_appearance(appearance: TerminalAppearance) -> bool {
    if terminal_appearance() == appearance {
        return false;
    }
    let imported = Arc::new(load_terminal_preferences_for(appearance));
    let mut guard = cache_guard();
    let current = guard
        .as_ref()
        .map(|cache| cache.appearance)
        .unwrap_or_default();
    if current == appearance {
        return false;
    }
    let overrides = guard
        .as_ref()
        .map(|cache| cache.overrides.clone())
        .unwrap_or_default();
    *guard = Some(rebuild_cache(imported, overrides, appearance));
    true
}

pub fn terminal_preference_overrides() -> TerminalPreferenceOverrides {
    cache_guard()
        .as_ref()
        .map(|cache| cache.overrides.clone())
        .unwrap_or_default()
}

/// Serializes tests, across modules, that mutate the process-wide preference cache.
#[cfg(test)]
pub(crate) fn preference_cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());
    CACHE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
        preference_cache_test_lock()
    }

    #[test]
    fn light_dark_pair_resolves_per_appearance() {
        for raw in ["light:Bright Day,dark:Deep Night", "dark:Deep Night,light:Bright Day"] {
            assert_eq!(
                theme_name_for_appearance(raw, TerminalAppearance::Light).as_deref(),
                Some("Bright Day"),
                "light appearance of {raw:?}"
            );
            assert_eq!(
                theme_name_for_appearance(raw, TerminalAppearance::Dark).as_deref(),
                Some("Deep Night"),
                "dark appearance of {raw:?}"
            );
        }
    }

    #[test]
    fn single_sided_pair_uses_the_declared_side() {
        for appearance in [TerminalAppearance::Light, TerminalAppearance::Dark] {
            assert_eq!(
                theme_name_for_appearance("dark:Deep Night", appearance).as_deref(),
                Some("Deep Night")
            );
            assert_eq!(
                theme_name_for_appearance("light:Bright Day", appearance).as_deref(),
                Some("Bright Day")
            );
        }
    }

    #[test]
    fn plain_theme_name_is_unchanged_for_both_appearances() {
        for appearance in [TerminalAppearance::Light, TerminalAppearance::Dark] {
            assert_eq!(
                theme_name_for_appearance("  tokyonight  ", appearance).as_deref(),
                Some("tokyonight")
            );
            assert_eq!(theme_name_for_appearance("   ", appearance), None);
        }
        assert_eq!(TerminalAppearance::default(), TerminalAppearance::Dark);
    }

    #[test]
    fn light_appearance_without_a_declared_theme_uses_the_light_palette() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("absent-config");
        let light = load_terminal_preferences_from_path_for(&missing, TerminalAppearance::Light);
        let dark = load_terminal_preferences_from_path_for(&missing, TerminalAppearance::Dark);
        assert_eq!(light.theme, TerminalThemeColors::light());
        assert_eq!(dark.theme, TerminalThemeColors::default());

        let fonts_only = temp.path().join("fonts-only");
        fs::write(&fonts_only, "font-size = 14\n").expect("write config");
        let light = load_terminal_preferences_from_path_for(&fonts_only, TerminalAppearance::Light);
        assert_eq!(light.theme.background, "#fafafa");
        assert_eq!(light.font_size, 14.0);

        // A declared background is the user's choice and is kept under both appearances.
        let declared = temp.path().join("declared");
        fs::write(&declared, "background = #101010\n").expect("write config");
        let light = load_terminal_preferences_from_path_for(&declared, TerminalAppearance::Light);
        assert_eq!(light.theme.background, "#101010");
    }

    #[test]
    fn set_terminal_appearance_reports_only_real_changes() {
        let _lock = cache_test_lock();
        set_terminal_appearance(TerminalAppearance::Dark);
        assert!(!set_terminal_appearance(TerminalAppearance::Dark));
        assert!(set_terminal_appearance(TerminalAppearance::Light));
        assert!(!set_terminal_appearance(TerminalAppearance::Light));
        assert_eq!(terminal_appearance(), TerminalAppearance::Light);
        assert!(set_terminal_appearance(TerminalAppearance::Dark));
        assert_eq!(terminal_appearance(), TerminalAppearance::Dark);
    }

    #[test]
    fn reload_keeps_the_pushed_appearance() {
        let _lock = cache_test_lock();
        set_terminal_appearance(TerminalAppearance::Light);
        reload_terminal_preferences();
        assert_eq!(terminal_appearance(), TerminalAppearance::Light);
        set_terminal_preference_overrides(TerminalPreferenceOverrides::default());
        assert_eq!(terminal_appearance(), TerminalAppearance::Light);
        assert!(set_terminal_appearance(TerminalAppearance::Dark));
    }

    #[test]
    fn test_empty_font_family_resets_list() {
        let config_str =
            "font-family = \"MesloLGS NF\"\nfont-family =\nfont-family = \"Fira Code\"";
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
        prefs_config.palette.insert(1, "#ff0000".to_string());
        for idx in 16u8..=255u8 {
            prefs_config.palette.insert(idx, format!("#color_{}", idx));
        }
        let prefs = TerminalPreferences::imported(prefs_config, PathBuf::from("test"));
        assert_eq!(prefs.theme.extended_ansi.len(), 240);
        assert_eq!(prefs.theme.extended_ansi[0], "#color_16");
        assert_eq!(prefs.theme.palette_overrides.len(), 241);
        assert_eq!(prefs.theme.palette_overrides[0], (1, "#ff0000".to_string()));
        assert_eq!(
            prefs.theme.palette_overrides[1],
            (16, "#color_16".to_string())
        );
        assert_eq!(
            prefs.theme.palette_overrides[240],
            (255, "#color_255".to_string())
        );
    }

    #[test]
    fn test_find_ghostty_binary_on_path_and_theme_candidates() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let bin_dir = temp_dir.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("create bin dir");
        let ghostty_bin = bin_dir.join(if cfg!(windows) {
            "ghostty.exe"
        } else {
            "ghostty"
        });
        fs::write(&ghostty_bin, b"#!/bin/sh\nexit 0\n").expect("write ghostty bin");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&ghostty_bin).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&ghostty_bin, perms).expect("set permissions");
        }

        struct EnvGuard {
            key: &'static str,
            orig: Option<std::ffi::OsString>,
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                if let Some(orig) = &self.orig {
                    env::set_var(self.key, orig);
                } else {
                    env::remove_var(self.key);
                }
            }
        }

        let _guard = EnvGuard {
            key: "PATH",
            orig: env::var_os("PATH"),
        };

        env::set_var("PATH", &bin_dir);

        let found = find_ghostty_binary_on_path();
        assert_eq!(found, Some(ghostty_bin.clone()));

        let candidates = theme_config_candidates("tokyonight", TerminalAppearance::Dark);
        let expected_bundle = bin_dir
            .join("..")
            .join("ghostty")
            .join("themes")
            .join("tokyonight");
        let expected_share = bin_dir
            .join("..")
            .join("share")
            .join("ghostty")
            .join("themes")
            .join("tokyonight");

        assert!(
            candidates.contains(&expected_bundle),
            "candidates {candidates:?} should contain {expected_bundle:?}"
        );
        assert!(
            candidates.contains(&expected_share),
            "candidates {candidates:?} should contain {expected_share:?}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&ghostty_bin).expect("metadata").permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&ghostty_bin, perms).expect("set permissions");
            assert_eq!(find_ghostty_binary_on_path(), None);
        }
    }

    #[test]
    fn test_ghostty_candidates_include_windows_roots() {
        // Windows defines neither HOME nor XDG_CONFIG_HOME: Ghostty keeps its config under
        // %LOCALAPPDATA%\ghostty there, with %APPDATA% as the secondary convention.
        let local_app_data = PathBuf::from(r"C:\Users\ferryx\AppData\Local");
        let app_data = PathBuf::from(r"C:\Users\ferryx\AppData\Roaming");
        let roots = GhosttyEnvRoots {
            home: None,
            xdg_config_home: None,
            local_app_data: Some(local_app_data.clone()),
            app_data: Some(app_data.clone()),
        };

        let config_dirs = roots.config_dirs();
        assert_eq!(config_dirs, vec![local_app_data.join("ghostty"), app_data.join("ghostty")]);
        assert!(
            config_dirs[0].to_string_lossy().starts_with(r"C:\Users\ferryx\AppData\Local"),
            "candidate {:?} should be a Windows-shaped path",
            config_dirs[0]
        );

        let config_candidates = config_candidates_from_roots(&roots);
        for expected in [
            local_app_data.join("ghostty").join("config.ghostty"),
            local_app_data.join("ghostty").join("config"),
        ] {
            assert!(
                config_candidates.contains(&expected),
                "candidates {config_candidates:?} should contain {expected:?}"
            );
        }

        let theme_candidates = theme_candidates_from_roots(&roots, "tokyonight");
        let expected_theme = local_app_data.join("ghostty").join("themes").join("tokyonight");
        assert!(
            theme_candidates.contains(&expected_theme),
            "candidates {theme_candidates:?} should contain {expected_theme:?}"
        );

        // A unix root keeps resolving through the same helper, with XDG_CONFIG_HOME first.
        let unix_roots = GhosttyEnvRoots {
            home: Some(PathBuf::from("/home/ferryx")),
            xdg_config_home: Some(PathBuf::from("/home/ferryx/.config")),
            local_app_data: None,
            app_data: None,
        };
        assert_eq!(
            unix_roots.config_dirs(),
            vec![
                PathBuf::from("/home/ferryx/.config").join("ghostty"),
                PathBuf::from("/home/ferryx").join(".config").join("ghostty"),
            ]
        );
    }

    #[test]
    fn test_apply_terminal_preference_overrides_shell() {
        let base = TerminalPreferences::defaults(TerminalPreferencesStatus::Absent, None);
        assert_eq!(base.default_shell, None);

        let overrides = TerminalPreferenceOverrides {
            shell: Some("pwsh".to_string()),
            ..Default::default()
        };
        let effective = apply_terminal_preference_overrides(&base, &overrides);
        assert_eq!(effective.default_shell.as_deref(), Some("pwsh"));

        let empty_overrides = TerminalPreferenceOverrides {
            shell: Some("   ".to_string()),
            ..Default::default()
        };
        let effective_empty = apply_terminal_preference_overrides(&base, &empty_overrides);
        assert_eq!(effective_empty.default_shell, None);
    }

    #[test]
    fn test_scrollback_parsing_and_overrides() {
        let config_text = r#"
font-family = "Fira Code"
scrollback-limit-lines = 25000
"#;
        let config = parse_ghostty_config(config_text).expect("parse config");
        assert_eq!(config.scrollback_limit, Some(25_000));

        let imported = TerminalPreferences::imported(config, PathBuf::from("/tmp/config"));
        assert_eq!(imported.scrollback, 25_000);

        let overrides = TerminalPreferenceOverrides {
            scrollback: Some(50_000),
            ..Default::default()
        };
        let effective = apply_terminal_preference_overrides(&imported, &overrides);
        assert_eq!(effective.scrollback, 50_000);
    }
}
