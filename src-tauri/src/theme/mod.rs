use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_THEME_FILE_BYTES: u64 = 616 * 1024;
pub const MAX_BACKGROUND_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

pub const BUILT_IN_THEME_IDS: &[&str] = &[
    "charcoal",
    "dark",
    "light",
    "system",
    "t3-chat",
    "grove",
    "ocean",
    "ember",
    "iris",
];

pub const RESERVED_PUBLISH_IDS: &[&str] = &[
    "system",
    "light",
    "dark",
    "charcoal",
    "t3-chat",
    "grove",
    "ocean",
    "ember",
    "iris",
    "t3-chat-dark",
    "t3-grove",
    "t3-ocean",
    "t3-ember",
    "t3-iris",
    "t3-code",
];

pub const THEME_GUIDE: &str = concat!(
    "Themes restyle Ferryx on desktop and web/mobile.\n\n",
    "Formats accepted by `ferryx-cli theme set <file.json>`:\n",
    "1. T3 Code shorthand: {\"name\":\"Nightfall\",\"appearance\":\"dark\",\"canvas\":\"#1a1b26\",\"accent\":\"#7aa2f7\",\"colors\":{...},\"css\":\"...\"}\n",
    "2. T3 Code v1: {\"version\":1,\"id\":\"nightfall\",\"name\":\"Nightfall\",\"appearance\":\"dark\",\"colors\":{...},\"variants\":{...},\"css\":\"...\"}\n",
    "3. Ferryx v1: {\"version\":1,\"id\":\"nightfall\",\"name\":\"Nightfall\",\"base\":\"dark\",\"tokens\":{\"--background\":\"#1a1b26\"},\"css\":\"...\"}\n\n",
    "CSS: `css` applies after the app's styles. Use `.dark` or `:root:not(.dark)` for one appearance.\n",
    "Remote fetches (@import, image-set(), non-data:/#id url()) are stripped at runtime.\n",
    "Background image: place `<themeId>.jpg`, `<themeId>.png`, or `<themeId>.webp` (<= 16 MB) beside `<themeId>.json`, then reference `var(--theme-background-image)` in CSS.\n\n",
    "Stable selectors:\n",
    "- [data-slot=\"sidebar\"]: app sidebar\n",
    "- [data-chat-header]: chat header\n",
    "- [data-workspace-area=\"conversation\"]: chat area behind messages\n",
    "- [data-message-role=\"user\"]: user message row\n",
    "- [data-user-message-bubble]: user message bubble\n",
    "- [data-slot=\"button\"]: button\n",
    "- [data-slot=\"input\"]: text input\n",
    "- [data-slot=\"switch\"]: switch\n",
    "- [data-slot=\"select-trigger\"]: select\n",
    "- [data-slot=\"dialog-popup\"]: dialog\n\n",
    "Undo: run `ferryx-cli theme set charcoal` or `ferryx-cli theme clear`.\n"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentThemeSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_theme_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_appearance: Option<String>,
    #[serde(default)]
    pub revision: u64,
}

impl Default for EnvironmentThemeSettings {
    fn default() -> Self {
        Self {
            default_theme_id: None,
            default_appearance: None,
            revision: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedThemeEntry {
    pub id: String,
    pub name: String,
    pub appearance: String,
    pub has_css: bool,
    pub has_background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_revision: Option<String>,
    pub document: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentThemesSnapshot {
    pub themes_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_theme_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_appearance: Option<String>,
    pub revision: u64,
    pub themes: Vec<PublishedThemeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeBackgroundAsset {
    pub path: PathBuf,
    pub mime_type: &'static str,
    pub revision: String,
}

pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_DATA_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
    {
        return PathBuf::from(home).join(".ferryx");
    }
    std::env::temp_dir().join("ferryx")
}

pub fn default_themes_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FERRYX_THEMES_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    default_data_dir().join("themes")
}

pub fn default_theme_settings_path() -> PathBuf {
    default_data_dir().join("theme-settings.json")
}

pub fn is_valid_theme_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() < 2 || bytes.len() > 64 {
        return false;
    }
    if !matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9') {
        return false;
    }
    bytes
        .iter()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

pub fn is_reserved_publish_id(id: &str) -> bool {
    RESERVED_PUBLISH_IDS.contains(&id)
}

pub fn is_built_in_theme_id(id: &str) -> bool {
    BUILT_IN_THEME_IDS.contains(&id)
}

pub fn is_valid_appearance(value: &str) -> bool {
    matches!(value, "light" | "dark" | "system")
}

pub fn derive_theme_id_from_str(input: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in input.trim().chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            slug.push(lower);
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').chars().take(48).collect::<String>();
    if trimmed.len() >= 2 {
        trimmed
    } else {
        format!("theme-{}", if trimmed.is_empty() { "custom" } else { &trimmed })
    }
}

fn validate_theme_json_value(value: &Value) -> Result<(String, String, bool), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "Theme file must contain a JSON object.".to_string())?;
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 64)
        .ok_or_else(|| "Theme file requires a non-empty `name` (<= 64 chars).".to_string())?
        .to_string();

    let appearance = obj
        .get("appearance")
        .or_else(|| obj.get("base"))
        .and_then(Value::as_str)
        .filter(|s| matches!(*s, "light" | "dark"))
        .ok_or_else(|| {
            "Theme file requires `appearance` (or `base`) set to \"light\" or \"dark\".".to_string()
        })?
        .to_string();

    let has_colors = obj.get("colors").and_then(Value::as_object).is_some()
        || obj.get("tokens").and_then(Value::as_object).is_some()
        || (obj.get("canvas").and_then(Value::as_str).is_some()
            && obj.get("accent").and_then(Value::as_str).is_some());
    if !has_colors {
        return Err(
            "Theme file must provide `colors`, `tokens`, or both `canvas` and `accent`.".to_string(),
        );
    }

    let has_css = obj
        .get("css")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());

    Ok((name, appearance, has_css))
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {e}", parent.display()))?;
    }
    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("theme"),
        std::process::id()
    );
    let tmp_path = path.with_file_name(tmp_name);
    {
        let mut file = fs::File::create(&tmp_path)
            .map_err(|e| format!("Failed to create temp file {}: {e}", tmp_path.display()))?;
        file.write_all(content)
            .map_err(|e| format!("Failed to write temp file {}: {e}", tmp_path.display()))?;
        let _ = file.sync_all();
    }
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to rename {} to {}: {e}", tmp_path.display(), path.display())
    })
}

pub fn read_theme_settings_in(settings_path: &Path) -> EnvironmentThemeSettings {
    let Ok(bytes) = fs::read(settings_path) else {
        return EnvironmentThemeSettings::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn write_theme_settings_in(
    settings_path: &Path,
    settings: &EnvironmentThemeSettings,
) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("Failed to serialize theme settings: {e}"))?;
    atomic_write(settings_path, &payload)
}

pub fn find_theme_background_in(themes_dir: &Path, theme_id: &str) -> Option<ThemeBackgroundAsset> {
    if !is_valid_theme_id(theme_id) {
        return None;
    }
    for (ext, mime) in [
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("png", "image/png"),
        ("webp", "image/webp"),
    ] {
        let candidate = themes_dir.join(format!("{theme_id}.{ext}"));
        let Ok(meta) = fs::metadata(&candidate) else {
            continue;
        };
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_BACKGROUND_IMAGE_BYTES {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        return Some(ThemeBackgroundAsset {
            path: candidate,
            mime_type: mime,
            revision: format!("{ext}:{}:{mtime}", meta.len()),
        });
    }
    None
}

pub fn read_theme_background_bytes_in(
    themes_dir: &Path,
    theme_id: &str,
) -> Result<(Vec<u8>, &'static str, String), String> {
    let asset = find_theme_background_in(themes_dir, theme_id)
        .ok_or_else(|| format!("No background image published for theme `{theme_id}`."))?;
    let bytes = fs::read(&asset.path)
        .map_err(|e| format!("Failed to read background image {}: {e}", asset.path.display()))?;
    if bytes.len() as u64 > MAX_BACKGROUND_IMAGE_BYTES {
        return Err("Background image exceeds 16 MiB limit.".to_string());
    }
    Ok((bytes, asset.mime_type, asset.revision))
}

pub fn read_published_themes_in(
    themes_dir: &Path,
    settings_path: &Path,
) -> EnvironmentThemesSnapshot {
    let settings = read_theme_settings_in(settings_path);
    let mut themes = Vec::new();
    if let Ok(entries) = fs::read_dir(themes_dir) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        files.sort();
        for path in files {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !is_valid_theme_id(stem) || is_reserved_publish_id(stem) {
                continue;
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_THEME_FILE_BYTES {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(mut doc) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Ok((name, appearance, has_css)) = validate_theme_json_value(&doc) else {
                continue;
            };
            if let Some(obj) = doc.as_object_mut() {
                obj.insert("id".to_string(), Value::String(stem.to_string()));
            }
            let bg = find_theme_background_in(themes_dir, stem);
            themes.push(PublishedThemeEntry {
                id: stem.to_string(),
                name,
                appearance,
                has_css,
                has_background: bg.is_some(),
                background_revision: bg.map(|b| b.revision),
                document: doc,
            });
        }
    }
    EnvironmentThemesSnapshot {
        themes_dir: themes_dir.to_string_lossy().into_owned(),
        default_theme_id: settings.default_theme_id,
        default_appearance: settings.default_appearance,
        revision: settings.revision,
        themes,
    }
}

pub fn set_environment_theme_in(
    themes_dir: &Path,
    settings_path: &Path,
    target: Option<&str>,
    appearance: Option<&str>,
) -> Result<EnvironmentThemesSnapshot, String> {
    if let Some(app) = appearance {
        if !is_valid_appearance(app) {
            return Err(format!(
                "Invalid appearance `{app}`. Expected \"light\", \"dark\", or \"system\"."
            ));
        }
    }
    if target.is_none() && appearance.is_none() {
        return Err("Provide a theme id/file path, `--appearance`, or both.".to_string());
    }

    let mut settings = read_theme_settings_in(settings_path);
    if let Some(target_str) = target {
        let target_path = Path::new(target_str);
        if target_path.is_file() {
            let meta = fs::metadata(target_path)
                .map_err(|e| format!("Failed to stat {}: {e}", target_path.display()))?;
            if meta.len() > MAX_THEME_FILE_BYTES {
                return Err(format!(
                    "Theme file exceeds maximum size ({MAX_THEME_FILE_BYTES} bytes)."
                ));
            }
            let raw = fs::read_to_string(target_path)
                .map_err(|e| format!("Failed to read {}: {e}", target_path.display()))?;
            let mut value: Value = serde_json::from_str(&raw)
                .map_err(|e| format!("Invalid theme JSON in {}: {e}", target_path.display()))?;
            let (name, _app, _css) = validate_theme_json_value(&value)?;
            let stem_id = target_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(derive_theme_id_from_str)
                .unwrap_or_else(|| derive_theme_id_from_str(&name));
            let theme_id = value
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| is_valid_theme_id(id))
                .map(ToOwned::to_owned)
                .unwrap_or(stem_id);
            if !is_valid_theme_id(&theme_id) {
                return Err(format!("Invalid theme id `{theme_id}`."));
            }
            if is_reserved_publish_id(&theme_id) {
                return Err(format!(
                    "Theme id `{theme_id}` is reserved and cannot be published."
                ));
            }
            if let Some(obj) = value.as_object_mut() {
                obj.insert("id".to_string(), Value::String(theme_id.clone()));
            }
            let normalized = serde_json::to_vec_pretty(&value)
                .map_err(|e| format!("Failed to serialize normalized theme: {e}"))?;
            let dest = themes_dir.join(format!("{theme_id}.json"));
            atomic_write(&dest, &normalized)?;
            settings.default_theme_id = Some(theme_id);
        } else {
            if !is_valid_theme_id(target_str) {
                return Err(format!(
                    "Unknown theme file and invalid theme id `{target_str}`."
                ));
            }
            let published_file = themes_dir.join(format!("{target_str}.json"));
            if !is_built_in_theme_id(target_str) && !published_file.is_file() {
                return Err(format!(
                    "Theme `{target_str}` is neither a built-in theme nor published in {}.",
                    themes_dir.display()
                ));
            }
            settings.default_theme_id = Some(target_str.to_string());
        }
    }

    if let Some(app) = appearance {
        settings.default_appearance = Some(app.to_string());
    }
    settings.revision = settings.revision.saturating_add(1);
    write_theme_settings_in(settings_path, &settings)?;
    Ok(read_published_themes_in(themes_dir, settings_path))
}

pub fn clear_environment_theme_in(
    themes_dir: &Path,
    settings_path: &Path,
) -> Result<EnvironmentThemesSnapshot, String> {
    let mut settings = read_theme_settings_in(settings_path);
    settings.default_theme_id = None;
    settings.default_appearance = None;
    settings.revision = settings.revision.saturating_add(1);
    write_theme_settings_in(settings_path, &settings)?;
    Ok(read_published_themes_in(themes_dir, settings_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn publishes_t3_shorthand_atomically_and_resolves_sidecar_background() {
        let dir = tempdir().expect("tempdir");
        let themes_dir = dir.path().join("themes");
        let settings_path = dir.path().join("theme-settings.json");
        let src_file = dir.path().join("nightfall.json");
        fs::write(
            &src_file,
            r##"{"name":"Nightfall","appearance":"dark","canvas":"#1a1b26","accent":"#7aa2f7"}"##,
        )
        .expect("write src");

        let snap = set_environment_theme_in(
            &themes_dir,
            &settings_path,
            Some(src_file.to_str().unwrap()),
            Some("dark"),
        )
        .expect("publish");
        assert_eq!(snap.default_theme_id.as_deref(), Some("nightfall"));
        assert_eq!(snap.default_appearance.as_deref(), Some("dark"));
        assert_eq!(snap.revision, 1);
        assert_eq!(snap.themes.len(), 1);
        assert_eq!(snap.themes[0].id, "nightfall");
        assert!(!snap.themes[0].has_background);

        fs::write(themes_dir.join("nightfall.png"), b"\x89PNG\r\n\x1a\nfake").expect("write png");
        let snap2 = read_published_themes_in(&themes_dir, &settings_path);
        assert!(snap2.themes[0].has_background);
        let (bytes, mime, _rev) =
            read_theme_background_bytes_in(&themes_dir, "nightfall").expect("bg bytes");
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"\x89PNG\r\n\x1a\nfake");

        let cleared = clear_environment_theme_in(&themes_dir, &settings_path).expect("clear");
        assert_eq!(cleared.default_theme_id, None);
        assert_eq!(cleared.default_appearance, None);
        assert_eq!(cleared.themes.len(), 1);
    }

    #[test]
    fn rejects_reserved_publish_ids_and_unknown_ids() {
        let dir = tempdir().expect("tempdir");
        let themes_dir = dir.path().join("themes");
        let settings_path = dir.path().join("theme-settings.json");
        let reserved_file = dir.path().join("dark.json");
        fs::write(
            &reserved_file,
            r##"{"id":"dark","name":"Dark","appearance":"dark","canvas":"#000","accent":"#fff"}"##,
        )
        .expect("write");
        let err = set_environment_theme_in(
            &themes_dir,
            &settings_path,
            Some(reserved_file.to_str().unwrap()),
            None,
        )
        .expect_err("reserved should fail");
        assert!(err.contains("reserved"));

        let missing = set_environment_theme_in(&themes_dir, &settings_path, Some("nope-id"), None)
            .expect_err("unknown id should fail");
        assert!(missing.contains("neither a built-in"));

        let ok = set_environment_theme_in(&themes_dir, &settings_path, Some("t3-chat"), None)
            .expect("built-in curated id allowed");
        assert_eq!(ok.default_theme_id.as_deref(), Some("t3-chat"));
    }
}
