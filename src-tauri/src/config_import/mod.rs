//! Pure, bounded importers. Imported actions, includes and scripts are never run.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod ghostty;
mod iterm2;
mod wezterm;
#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn ghostty_maps_font_theme_cursor() {
    tests::ghostty_maps_font_theme_cursor();
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub scrollback: Option<usize>,
    pub cursor_style: Option<String>,
    pub theme: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Preview {
    pub settings: Settings,
    pub skipped: Vec<String>,
}

pub fn parse(format: &str, bytes: &[u8]) -> Result<Preview, String> {
    if bytes.len() > 1024 * 1024 { return Err("IMPORT_PARSE_ERROR".into()); }
    let result = match format {
        "ghostty" => ghostty::parse(std::str::from_utf8(bytes).map_err(|_| "IMPORT_PARSE_ERROR")?),
        "iterm2" => iterm2::parse(bytes),
        "wezterm" => wezterm::parse(std::str::from_utf8(bytes).map_err(|_| "IMPORT_PARSE_ERROR")?),
        _ => Err("IMPORT_PARSE_ERROR".into()),
    };
    result.map_err(|_| "IMPORT_PARSE_ERROR".into())
}

pub(super) fn color(value: &str) -> Option<String> {
    let value = value.strip_prefix('#').unwrap_or(value);
    (value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{value}"))
}

pub(super) const ANSI: [&str; 16] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"];

impl Settings {
    pub(super) fn number(&mut self, key: &str, value: &str) -> bool {
        match key {
            "font-size" => value.parse::<f32>().ok().filter(|n| n.is_finite() && (10.0..=36.0).contains(n))
                .map(|n| self.font_size = Some(n)).is_some(),
            "scrollback" => value.parse::<usize>().ok().filter(|n| (1000..=100_000).contains(n))
                .map(|n| self.scrollback = Some(n)).is_some(),
            _ => false,
        }
    }
}

#[tauri::command]
pub async fn cmd_config_import(format: String, bytes: Vec<u8>) -> Result<Preview, String> {
    // No file access, command execution, or settings mutation; the UI confirms a preview.
    crate::ipc::run_blocking(move || parse(&format, &bytes).map_err(crate::ipc::IpcError::internal))
        .await.map_err(|_| "IMPORT_PARSE_ERROR".into())
}
