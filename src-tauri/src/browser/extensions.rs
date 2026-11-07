//! Extensions installed in a Chromium engine profile. While the profile's engine runs, the
//! list and every change go through the engine's control channel (its hidden
//! `chrome://extensions` page, see `cef_engine::extensions`); otherwise the list is read from
//! the profile's files on disk, which are never written here.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};

/// Icons larger than this are left out of the listing.
const MAX_ICON_BYTES: u64 = 256 * 1024;
/// Engine icons arrive as data URLs; longer ones (base64 of [`MAX_ICON_BYTES`]) are left out.
const MAX_ICON_DATA_URL_LEN: usize = (MAX_ICON_BYTES as usize) * 4 / 3 + 64;
/// Chromium's `Manifest::Location` values for extensions built into the browser.
const COMPONENT_LOCATIONS: [i64; 2] = [5, 10];
/// `Manifest::Location::INTERNAL`, an extension installed from a package (the Web Store).
const INTERNAL_LOCATION: i64 = 1;
const WEB_STORE_UPDATE_HOST: &str = "clients2.google.com";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub icon_data_url: Option<String>,
    pub options_url: Option<String>,
    pub homepage_url: Option<String>,
    pub from_web_store: bool,
    /// Whether the toolbar button is pinned; `None` when unknown (read from files).
    pub pinned: Option<bool>,
    /// False for extensions Chrome will not let the user remove (policy installed).
    pub removable: bool,
}

/// A Chrome page that manages extensions.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum ExtensionPage {
    Manage,
    Details(String),
    Options(String),
}

/// Chromium extension ids are 32 letters from `a` to `p`.
pub fn is_valid_extension_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

/// The URL that opens `page`. `options_url` is the extension's own options page, if it has one.
pub fn page_url(page: &ExtensionPage, options_url: Option<&str>) -> Result<String, String> {
    let checked = |id: &str| {
        if is_valid_extension_id(id) {
            Ok(())
        } else {
            Err(format!("invalid extension id: {id}"))
        }
    };
    match page {
        ExtensionPage::Manage => Ok("chrome://extensions/".to_string()),
        ExtensionPage::Details(id) => {
            checked(id)?;
            Ok(format!("chrome://extensions/?id={id}"))
        }
        ExtensionPage::Options(id) => {
            checked(id)?;
            let own_prefix = format!("chrome-extension://{id}/");
            Ok(match options_url {
                Some(url) if url.starts_with(&own_prefix) => url.to_string(),
                _ => format!("chrome://extensions/?options={id}"),
            })
        }
    }
}

/// The extensions installed in the Chromium data dir `profile_root`, sorted by name.
pub fn list_installed(profile_root: &Path) -> std::io::Result<Vec<InstalledExtension>> {
    let profile = chromium_profile_dir(profile_root);
    let extensions_root = profile.join("Extensions");
    let settings = read_settings(&profile);
    let mut extensions = if settings.is_empty() {
        scan_extensions_dir(&extensions_root)?
    } else {
        settings
            .iter()
            .filter_map(|(id, entry)| from_settings(&extensions_root, id, entry))
            .collect()
    };
    sort_by_name(&mut extensions);
    Ok(extensions)
}

fn sort_by_name(extensions: &mut [InstalledExtension]) {
    extensions.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// The extensions in the engine's list answer (a JSON array built by its extensions page),
/// sorted by name. Entries without a valid id are dropped; icons that are not image data URLs
/// or are too large, options pages of another extension and non-web homepages are left out.
pub fn from_engine_list(list: &Value) -> Vec<InstalledExtension> {
    let mut extensions: Vec<InstalledExtension> = list
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(from_engine_entry)
        .collect();
    sort_by_name(&mut extensions);
    extensions
}

fn from_engine_entry(entry: &Value) -> Option<InstalledExtension> {
    let id = entry.get("id")?.as_str()?;
    if !is_valid_extension_id(id) {
        return None;
    }
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let flag = |key: &str| entry.get(key).and_then(Value::as_bool);
    let own_prefix = format!("chrome-extension://{id}/");
    Some(InstalledExtension {
        id: id.to_string(),
        name: text("name").unwrap_or_else(|| id.to_string()),
        version: text("version").unwrap_or_default(),
        description: text("description"),
        enabled: flag("enabled").unwrap_or(false),
        icon_data_url: text("iconDataUrl")
            .filter(|url| url.starts_with("data:image/") && url.len() <= MAX_ICON_DATA_URL_LEN),
        options_url: text("optionsUrl").filter(|url| url.starts_with(&own_prefix)),
        homepage_url: text("homepageUrl")
            .filter(|url| url.starts_with("https://") || url.starts_with("http://")),
        from_web_store: flag("fromWebStore").unwrap_or(false),
        pinned: flag("pinnedToToolbar"),
        removable: flag("removable").unwrap_or(false),
    })
}

/// Chromium keeps a profile in `Default` under its data dir.
fn chromium_profile_dir(root: &Path) -> PathBuf {
    let default = root.join("Default");
    if default.join("Preferences").is_file() || default.join("Extensions").is_dir() {
        default
    } else {
        root.to_path_buf()
    }
}

/// `extensions.settings` from `Preferences` and `Secure Preferences`, merged per extension.
fn read_settings(profile: &Path) -> Map<String, Value> {
    let mut merged = Map::new();
    for file in ["Preferences", "Secure Preferences"] {
        let Some(json) = read_json(&profile.join(file)) else {
            continue;
        };
        let Some(Value::Object(settings)) = json.pointer("/extensions/settings") else {
            continue;
        };
        for (id, entry) in settings {
            match (merged.get_mut(id), entry) {
                (Some(Value::Object(existing)), Value::Object(fields)) => {
                    existing.extend(fields.iter().map(|(key, value)| (key.clone(), value.clone())));
                }
                _ => {
                    merged.insert(id.clone(), entry.clone());
                }
            }
        }
    }
    merged
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn from_settings(extensions_root: &Path, id: &str, entry: &Value) -> Option<InstalledExtension> {
    if !is_valid_extension_id(id) {
        return None;
    }
    let location = entry.get("location").and_then(Value::as_i64);
    if location.is_some_and(|location| COMPONENT_LOCATIONS.contains(&location)) {
        return None;
    }
    let manifest_dir = match entry.get("path").and_then(Value::as_str).map(Path::new) {
        Some(path) if path.is_relative() => extensions_root.join(path),
        // An unpacked extension keeps its absolute source directory.
        Some(path) if path.join("manifest.json").is_file() => path.to_path_buf(),
        _ => highest_version_dir(&extensions_root.join(id))?,
    };
    let manifest = read_json(&manifest_dir.join("manifest.json"))?;
    let disabled_state = entry.get("state").and_then(Value::as_i64) == Some(0);
    let disable_reasons = match entry.get("disable_reasons") {
        Some(Value::Array(reasons)) => !reasons.is_empty(),
        Some(Value::Number(reasons)) => reasons.as_i64() != Some(0),
        _ => false,
    };
    let update_url = manifest
        .get("update_url")
        .or_else(|| entry.pointer("/manifest/update_url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let from_web_store = entry.get("from_webstore").and_then(Value::as_bool).unwrap_or(false)
        || (location == Some(INTERNAL_LOCATION) && update_url.contains(WEB_STORE_UPDATE_HOST));
    Some(describe(
        id,
        &manifest_dir,
        &manifest,
        !(disabled_state || disable_reasons),
        from_web_store,
    ))
}

/// Without preferences, every `Extensions/<id>/<version>` with a manifest counts as enabled.
fn scan_extensions_dir(extensions_root: &Path) -> std::io::Result<Vec<InstalledExtension>> {
    let entries = match std::fs::read_dir(extensions_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut extensions = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if id == "Temp" || !is_valid_extension_id(&id) {
            continue;
        }
        let Some(dir) = highest_version_dir(&entry.path()) else {
            continue;
        };
        let Some(manifest) = read_json(&dir.join("manifest.json")) else {
            continue;
        };
        let update_url = manifest.get("update_url").and_then(Value::as_str).unwrap_or_default();
        let from_web_store = update_url.contains(WEB_STORE_UPDATE_HOST);
        extensions.push(describe(&id, &dir, &manifest, true, from_web_store));
    }
    Ok(extensions)
}

/// The highest-versioned subdirectory (`1.2.10_0` beats `1.2.9_0`) that has a manifest.
fn highest_version_dir(extension_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(extension_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("manifest.json").is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            Some((version_key(&name), entry.path()))
        })
        .max_by(|(a, _), (b, _)| compare_versions(a, b))
        .map(|(_, path)| path)
}

fn version_key(name: &str) -> Vec<u64> {
    name.split(['.', '_'])
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn compare_versions(a: &[u64], b: &[u64]) -> Ordering {
    let len = a.len().max(b.len());
    (0..len)
        .map(|index| {
            let left = a.get(index).copied().unwrap_or(0);
            let right = b.get(index).copied().unwrap_or(0);
            left.cmp(&right)
        })
        .find(|ordering| ordering.is_ne())
        .unwrap_or(Ordering::Equal)
}

fn describe(
    id: &str,
    dir: &Path,
    manifest: &Value,
    enabled: bool,
    from_web_store: bool,
) -> InstalledExtension {
    let text = |key: &str| manifest.get(key).and_then(Value::as_str);
    let name = text("name")
        .and_then(|name| localize(dir, manifest, name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_string());
    let description = text("description")
        .and_then(|description| localize(dir, manifest, description))
        .filter(|description| !description.is_empty());
    let options_page = manifest
        .pointer("/options_ui/page")
        .and_then(Value::as_str)
        .or_else(|| text("options_page"))
        .map(|page| page.trim_start_matches('/'))
        .filter(|page| !page.is_empty());
    InstalledExtension {
        id: id.to_string(),
        name,
        version: text("version").unwrap_or_default().to_string(),
        description,
        enabled,
        icon_data_url: icon_data_url(dir, manifest),
        options_url: options_page.map(|page| format!("chrome-extension://{id}/{page}")),
        homepage_url: text("homepage_url").map(str::to_string),
        from_web_store,
        // The files do not say whether the button is pinned. Chrome refuses removing a
        // policy-installed extension itself and reports why.
        pinned: None,
        removable: true,
    }
}

/// Resolves a `__MSG_key__` string from the default locale's messages; other text is returned
/// as is. `None` when the message cannot be found.
fn localize(dir: &Path, manifest: &Value, value: &str) -> Option<String> {
    let Some(key) = value.strip_prefix("__MSG_").and_then(|rest| rest.strip_suffix("__")) else {
        return Some(value.to_string());
    };
    let locale = manifest.get("default_locale").and_then(Value::as_str)?;
    if !is_plain_relative(Path::new(locale)) {
        return None;
    }
    let messages = read_json(&dir.join("_locales").join(locale).join("messages.json"))?;
    let key = key.to_lowercase();
    messages
        .as_object()?
        .iter()
        .find(|(name, _)| name.to_lowercase() == key)
        .and_then(|(_, message)| message.get("message")?.as_str())
        .map(str::to_string)
}

fn icon_data_url(dir: &Path, manifest: &Value) -> Option<String> {
    let path = manifest
        .get("icons")
        .and_then(largest_icon)
        .or_else(|| manifest.pointer("/action/default_icon").and_then(largest_icon))
        .or_else(|| manifest.pointer("/browser_action/default_icon").and_then(largest_icon))?;
    let relative = Path::new(path.trim_start_matches('/'));
    if !is_plain_relative(relative) {
        return None;
    }
    let mime = match relative.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => return None,
    };
    let file = dir.join(relative);
    if std::fs::metadata(&file).ok()?.len() > MAX_ICON_BYTES {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

/// An icon path, or the one with the largest numeric size key.
fn largest_icon(icons: &Value) -> Option<&str> {
    match icons {
        Value::String(path) => Some(path.as_str()),
        Value::Object(sizes) => sizes
            .iter()
            .filter_map(|(size, path)| Some((size.parse::<u32>().ok()?, path.as_str()?)))
            .max_by_key(|(size, _)| *size)
            .map(|(_, path)| path),
        _ => None,
    }
}

/// A path that stays inside the directory it is joined to.
fn is_plain_relative(path: &Path) -> bool {
    path.components().all(|component| matches!(component, Component::Normal(_)))
        && path.components().next().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    const STORE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DISABLED_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const COMPONENT_ID: &str = "cccccccccccccccccccccccccccccccc";
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G'];

    fn write_json(path: &Path, value: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn write_manifest(extensions: &Path, id: &str, version: &str, manifest: &Value) -> PathBuf {
        let dir = extensions.join(id).join(version);
        write_json(&dir.join("manifest.json"), manifest);
        dir
    }

    #[test]
    fn lists_extensions_from_preferences() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("Default");
        let extensions = profile.join("Extensions");
        let store_dir = write_manifest(
            &extensions,
            STORE_ID,
            "1.0.0_0",
            &json!({
                "name": "__MSG_appName__",
                "description": "__MSG_appDesc__",
                "version": "1.0.0",
                "default_locale": "en",
                "icons": { "16": "icons/16.png", "128": "/icons/128.png" },
                "options_ui": { "page": "/options.html" },
                "homepage_url": "https://example.com",
                "update_url": "https://clients2.google.com/service/update2/crx",
            }),
        );
        write_json(
            &store_dir.join("_locales/en/messages.json"),
            &json!({ "APPNAME": { "message": "Zeta Store" }, "appDesc": { "message": "Does things" } }),
        );
        fs::create_dir_all(store_dir.join("icons")).unwrap();
        fs::write(store_dir.join("icons/128.png"), PNG).unwrap();
        fs::write(store_dir.join("icons/16.png"), b"small").unwrap();
        write_manifest(&extensions, DISABLED_ID, "2.0_0", &json!({ "name": "alpha", "version": "2.0" }));
        write_manifest(&extensions, COMPONENT_ID, "1_0", &json!({ "name": "Built in", "version": "1" }));
        write_json(
            &profile.join("Preferences"),
            &json!({ "extensions": { "settings": {
                STORE_ID: { "location": 1, "path": format!("{STORE_ID}/1.0.0_0"), "state": 1 },
                COMPONENT_ID: { "location": 5, "path": format!("{COMPONENT_ID}/1_0") },
            } } }),
        );
        write_json(
            &profile.join("Secure Preferences"),
            &json!({ "extensions": { "settings": {
                DISABLED_ID: { "location": 4, "disable_reasons": [1] },
            } } }),
        );
        fs::write(profile.join("Secure Preferences.bak"), b"not json").unwrap();

        let listed = list_installed(root.path()).unwrap();
        let ids: Vec<&str> = listed.iter().map(|extension| extension.id.as_str()).collect();
        assert_eq!(ids, [DISABLED_ID, STORE_ID]);

        let disabled = &listed[0];
        assert!(!disabled.enabled);
        assert!(!disabled.from_web_store);
        assert_eq!(disabled.version, "2.0");
        assert_eq!(disabled.options_url, None);

        let store = &listed[1];
        assert!(store.enabled);
        assert!(store.from_web_store);
        assert_eq!(store.name, "Zeta Store");
        assert_eq!(store.description.as_deref(), Some("Does things"));
        assert_eq!(
            store.icon_data_url.as_deref(),
            Some(format!("data:image/png;base64,{}", STANDARD.encode(PNG)).as_str())
        );
        assert_eq!(
            store.options_url.as_deref(),
            Some(format!("chrome-extension://{STORE_ID}/options.html").as_str())
        );
        assert_eq!(store.homepage_url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn scans_extension_dirs_without_preferences() {
        let root = tempfile::tempdir().unwrap();
        let extensions = root.path().join("Extensions");
        write_manifest(&extensions, STORE_ID, "1.2.9_0", &json!({ "name": "Old", "version": "1.2.9" }));
        write_manifest(&extensions, STORE_ID, "1.2.10_0", &json!({ "name": "New", "version": "1.2.10" }));
        write_manifest(&extensions, "Temp", "1_0", &json!({ "name": "Temp", "version": "1" }));
        write_manifest(&extensions, "not-an-id", "1_0", &json!({ "name": "Bad", "version": "1" }));

        let listed = list_installed(root.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, STORE_ID);
        assert_eq!(listed[0].version, "1.2.10");
        assert!(listed[0].enabled);
    }

    #[test]
    fn missing_profile_lists_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert!(list_installed(&root.path().join("absent")).unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_extension_ids() {
        assert!(is_valid_extension_id(STORE_ID));
        assert!(!is_valid_extension_id(""));
        assert!(!is_valid_extension_id("qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq"));
        assert!(!is_valid_extension_id("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
        assert!(!is_valid_extension_id(&STORE_ID[..31]));
        assert!(page_url(&ExtensionPage::Details("../x".into()), None).is_err());
        assert!(page_url(&ExtensionPage::Options("zz".into()), None).is_err());
    }

    #[test]
    fn page_urls_for_each_page() {
        assert_eq!(page_url(&ExtensionPage::Manage, None).unwrap(), "chrome://extensions/");
        assert_eq!(
            page_url(&ExtensionPage::Details(STORE_ID.into()), None).unwrap(),
            format!("chrome://extensions/?id={STORE_ID}")
        );
        let own = format!("chrome-extension://{STORE_ID}/options.html");
        assert_eq!(page_url(&ExtensionPage::Options(STORE_ID.into()), Some(&own)).unwrap(), own);
        let foreign = format!("chrome-extension://{DISABLED_ID}/options.html");
        let fallback = format!("chrome://extensions/?options={STORE_ID}");
        assert_eq!(page_url(&ExtensionPage::Options(STORE_ID.into()), Some(&foreign)).unwrap(), fallback);
        assert_eq!(page_url(&ExtensionPage::Options(STORE_ID.into()), None).unwrap(), fallback);
    }

    #[test]
    fn deserializes_pages_from_tagged_json() {
        let manage: ExtensionPage = serde_json::from_value(json!({ "kind": "manage" })).unwrap();
        assert_eq!(manage, ExtensionPage::Manage);
        let options: ExtensionPage =
            serde_json::from_value(json!({ "kind": "options", "id": STORE_ID })).unwrap();
        assert_eq!(options, ExtensionPage::Options(STORE_ID.into()));
    }

    #[test]
    fn reads_the_engine_list() {
        let icon = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
        let listed = from_engine_list(&json!([
            {
                "id": STORE_ID,
                "name": "Zeta",
                "version": "1.2",
                "description": "Blocks ads",
                "enabled": true,
                "iconDataUrl": icon.clone(),
                "pinnedToToolbar": true,
                "optionsUrl": format!("chrome-extension://{STORE_ID}/options.html"),
                "homepageUrl": "https://example.com",
                "fromWebStore": true,
                "removable": true,
            },
            {
                "id": DISABLED_ID,
                "name": "alpha",
                "version": "2",
                "description": null,
                "enabled": false,
                "iconDataUrl": null,
                "pinnedToToolbar": false,
                "optionsUrl": null,
                "homepageUrl": null,
                "fromWebStore": false,
                "removable": false,
            },
        ]));
        assert_eq!(
            listed,
            vec![
                InstalledExtension {
                    id: DISABLED_ID.into(),
                    name: "alpha".into(),
                    version: "2".into(),
                    description: None,
                    enabled: false,
                    icon_data_url: None,
                    options_url: None,
                    homepage_url: None,
                    from_web_store: false,
                    pinned: Some(false),
                    removable: false,
                },
                InstalledExtension {
                    id: STORE_ID.into(),
                    name: "Zeta".into(),
                    version: "1.2".into(),
                    description: Some("Blocks ads".into()),
                    enabled: true,
                    icon_data_url: Some(icon),
                    options_url: Some(format!("chrome-extension://{STORE_ID}/options.html")),
                    homepage_url: Some("https://example.com".into()),
                    from_web_store: true,
                    pinned: Some(true),
                    removable: true,
                },
            ]
        );
        let serialized = serde_json::to_value(&listed[0]).unwrap();
        assert_eq!(serialized["pinned"], json!(false));
        assert_eq!(serialized["removable"], json!(false));
        assert_eq!(serialized["iconDataUrl"], Value::Null);
    }

    #[test]
    fn engine_entries_with_missing_fields_get_safe_defaults() {
        let listed = from_engine_list(&json!([{ "id": STORE_ID }]));
        assert_eq!(listed.len(), 1);
        let only = &listed[0];
        assert_eq!(only.name, STORE_ID);
        assert_eq!(only.version, "");
        assert!(!only.enabled);
        assert_eq!(only.pinned, None);
        assert!(!only.removable);
        assert!(!only.from_web_store);
        assert!(from_engine_list(&json!({ "not": "a list" })).is_empty());
        assert!(from_engine_list(&Value::Null).is_empty());
    }

    #[test]
    fn engine_entries_with_bad_ids_and_unsafe_urls_are_filtered() {
        let listed = from_engine_list(&json!([
            { "id": "../../etc", "name": "Bad" },
            { "id": STORE_ID.to_uppercase(), "name": "Upper" },
            { "name": "No id" },
            "not an object",
            {
                "id": STORE_ID,
                "name": "Kept",
                "iconDataUrl": "chrome://extension-icon/x/128/0",
                "optionsUrl": format!("chrome-extension://{DISABLED_ID}/options.html"),
                "homepageUrl": "javascript:alert(1)",
            },
        ]));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, STORE_ID);
        assert_eq!(listed[0].icon_data_url, None);
        assert_eq!(listed[0].options_url, None);
        assert_eq!(listed[0].homepage_url, None);
    }

    #[test]
    fn an_oversized_engine_icon_is_dropped() {
        let huge = format!("data:image/png;base64,{}", "A".repeat(MAX_ICON_DATA_URL_LEN));
        let listed = from_engine_list(&json!([{ "id": STORE_ID, "iconDataUrl": huge }]));
        assert_eq!(listed[0].icon_data_url, None);
    }
}
