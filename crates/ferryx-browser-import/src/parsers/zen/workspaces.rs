//! Zen Browser workspace parser.
//!
//! Ports `maho-chromium/browser/importer/zen_workspaces_parser.cc` to Rust.
//! Reads `<profile_dir>/zen-sessions.jsonlz4` (mozLz4 compressed) and extracts
//! workspaces, folders, and tabs into the shared types.

use std::path::Path;

use serde_json::Value;

use crate::{ImportError, ImportResult, ParsedFolder, ParsedTab, ParsedWorkspace};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZenParseStatus {
    Success,
    DecompressFailed,
    JsonParseFailed,
    MissingRequiredFields,
}

#[derive(Clone, Debug)]
pub struct ZenParseResult {
    pub status: ZenParseStatus,
    pub workspaces: Vec<ParsedWorkspace>,
    pub active_workspace_uuid: String,
}

impl Default for ZenParseResult {
    fn default() -> Self {
        Self {
            status: ZenParseStatus::Success,
            workspaces: Vec::new(),
            active_workspace_uuid: String::new(),
        }
    }
}

pub fn parse_zen_workspaces(profile_dir: &Path) -> ImportResult<ZenParseResult> {
    let sessions_path = profile_dir.join("zen-sessions.jsonlz4");

    let raw = std::fs::read(&sessions_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ImportError::FileNotFound(sessions_path.display().to_string())
        } else {
            ImportError::Io(format!("{}: {}", sessions_path.display(), e))
        }
    })?;

    let json_str = decompress_mozlz4(&raw).ok_or_else(|| {
        ImportError::DecompressFailed(format!("Failed to decompress {}", sessions_path.display()))
    })?;

    let root: Value = serde_json::from_str(&json_str)
        .map_err(|e| ImportError::Parse(format!("zen-sessions JSON: {}", e)))?;

    let root_obj = root
        .as_object()
        .ok_or_else(|| ImportError::Parse("zen-sessions root is not a JSON object".into()))?;

    let spaces_list = root_obj
        .get("spaces")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ImportError::Parse("missing 'spaces' array".into()))?;

    let tabs_list = root_obj
        .get("tabs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ImportError::Parse("missing 'tabs' array".into()))?;

    let folders_list = root_obj.get("folders").and_then(|v| v.as_array());

    let mut result = ZenParseResult::default();

    for space_val in spaces_list {
        let Some(space_dict) = space_val.as_object() else {
            continue;
        };

        let Some(uuid) = space_dict.get("uuid").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(name) = space_dict.get("name").and_then(|v| v.as_str()) else {
            continue;
        };

        let user_context_id = space_dict
            .get("userContextId")
            .or_else(|| space_dict.get("containerTabId"))
            .and_then(|v| match v {
                Value::Number(n) => n.as_i64(),
                _ => None,
            })
            .unwrap_or(0) as i32;

        let icon = space_dict
            .get("icon")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let theme_color_hex = extract_theme_color(space_dict);

        result.workspaces.push(ParsedWorkspace {
            uuid: uuid.to_string(),
            name: name.to_string(),
            icon,
            theme_color_hex,
            container_id: user_context_id,
            pinned_tabs: Vec::new(),
            favorites: Vec::new(),
            regular_tabs: Vec::new(),
            folders: Vec::new(),
        });
    }

    if let Some(folders) = folders_list {
        let mut raw_folders: Vec<ParsedFolder> = Vec::new();
        for folder_val in folders {
            let Some(folder_dict) = folder_val.as_object() else {
                continue;
            };
            let Some(id) = folder_dict.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(name) = folder_dict.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(ws_id) = folder_dict.get("workspaceId").and_then(|v| v.as_str()) else {
                continue;
            };
            let parent_id = folder_dict
                .get("parentId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            raw_folders.push(ParsedFolder {
                id: id.to_string(),
                name: name.to_string(),
                parent_id,
                workspace_id: ws_id.to_string(),
                tabs: Vec::new(),
            });
        }

        for ws in &mut result.workspaces {
            let ws_folders: Vec<ParsedFolder> = raw_folders
                .iter()
                .filter(|f| f.workspace_id == ws.uuid)
                .cloned()
                .collect();

            // Validate folder depth (max 5) and detect cycles.
            let valid: Vec<bool> = ws_folders
                .iter()
                .map(|folder| {
                    let mut depth = 0;
                    let mut current_parent = folder.parent_id.clone();
                    while !current_parent.is_empty() {
                        depth += 1;
                        if depth >= 5 {
                            return false;
                        }
                        let found = ws_folders.iter().find(|p| p.id == current_parent);
                        match found {
                            Some(parent) => {
                                if parent.id == folder.id {
                                    return false;
                                }
                                current_parent = parent.parent_id.clone();
                            }
                            None => break,
                        }
                    }
                    true
                })
                .collect();

            for (i, f) in ws_folders.into_iter().enumerate() {
                if valid[i] {
                    ws.folders.push(f);
                }
            }
        }
    }

    for tab_val in tabs_list {
        let Some(tab_dict) = tab_val.as_object() else {
            continue;
        };

        let Some(workspace_id) = tab_dict.get("zenWorkspace").and_then(|v| v.as_str()) else {
            continue;
        };
        if workspace_id.is_empty() {
            continue;
        }

        let url = extract_url_from_tab(tab_dict);
        if url.is_empty() || url == "about:blank" || url == "about:newtab" {
            continue;
        }

        let title = extract_title_from_tab(tab_dict);
        let is_pinned = tab_dict
            .get("pinned")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let is_essential = tab_dict
            .get("zenEssential")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let tab = ParsedTab {
            url,
            title,
            is_pinned,
            is_essential,
        };

        let folder_id = tab_dict
            .get("folderId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !folder_id.is_empty() {
            let mut found_folder = false;
            for ws in &mut result.workspaces {
                if ws.uuid == workspace_id {
                    for folder in &mut ws.folders {
                        if folder.id == folder_id {
                            folder.tabs.push(tab.clone());
                            found_folder = true;
                            break;
                        }
                    }
                    break;
                }
            }
            if found_folder {
                continue;
            }
        }

        for ws in &mut result.workspaces {
            if ws.uuid == workspace_id {
                if tab.is_essential {
                    ws.favorites.push(tab);
                } else if tab.is_pinned {
                    ws.pinned_tabs.push(tab);
                } else {
                    ws.regular_tabs.push(tab);
                }
                break;
            }
        }
    }

    result.active_workspace_uuid = read_active_workspace_from_prefs(profile_dir);
    Ok(result)
}

fn extract_url_from_tab(tab: &serde_json::Map<String, Value>) -> String {
    let entries = match tab.get("entries").and_then(|v| v.as_array()) {
        Some(e) if !e.is_empty() => e,
        _ => return String::new(),
    };
    entries
        .last()
        .and_then(|e| e.as_object())
        .and_then(|d| d.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_title_from_tab(tab: &serde_json::Map<String, Value>) -> String {
    let entries = match tab.get("entries").and_then(|v| v.as_array()) {
        Some(e) if !e.is_empty() => e,
        _ => return String::new(),
    };
    entries
        .last()
        .and_then(|e| e.as_object())
        .and_then(|d| d.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_theme_color(space: &serde_json::Map<String, Value>) -> String {
    space
        .get("theme")
        .and_then(|v| v.as_object())
        .and_then(|t| t.get("gradientColors"))
        .and_then(|v| v.as_array())
        .and_then(|colors| colors.first())
        .and_then(|c| c.as_object())
        .and_then(|d| d.get("color"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn read_active_workspace_from_prefs(profile_dir: &Path) -> String {
    let prefs_path = profile_dir.join("prefs.js");
    let content = match std::fs::read_to_string(&prefs_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    const PREF_KEY: &str = "\"zen.workspaces.active\", \"";
    let pos = match content.find(PREF_KEY) {
        Some(p) => p + PREF_KEY.len(),
        None => return String::new(),
    };

    let rest = &content[pos..];
    match rest.find('"') {
        Some(end) => rest[..end].to_string(),
        None => String::new(),
    }
}

// --- mozLz4 decompression ---

const MOZLZ4_MAGIC: &[u8; 8] = b"mozLz40\0";
const HEADER_SIZE: usize = 8 + 4; // magic + LE u32 size

fn decompress_mozlz4(data: &[u8]) -> Option<String> {
    if data.len() < HEADER_SIZE {
        return None;
    }

    if &data[..8] != MOZLZ4_MAGIC {
        return None;
    }

    let uncompressed_size = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

    const MAX_DECOMPRESSED: usize = 256 * 1024 * 1024;
    if uncompressed_size > MAX_DECOMPRESSED {
        return None;
    }

    let compressed = &data[HEADER_SIZE..];
    let decompressed = lz4_flex::block::decompress(compressed, uncompressed_size).ok()?;

    String::from_utf8(decompressed).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mozlz4(json: &str) -> Vec<u8> {
        let compressed = lz4_flex::block::compress(json.as_bytes());
        let size = json.len() as u32;
        let mut data = Vec::new();
        data.extend_from_slice(MOZLZ4_MAGIC);
        data.extend_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&compressed);
        data
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err = parse_zen_workspaces(tmp.path()).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {:?}", err);
    }

    #[test]
    fn corrupted_data_returns_decompress_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("zen-sessions.jsonlz4"), b"garbage data").unwrap();
        let err = parse_zen_workspaces(tmp.path()).unwrap_err();
        assert!(
            matches!(err, ImportError::DecompressFailed(_)),
            "got {:?}",
            err
        );
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let tmp = tempfile::tempdir().unwrap();
        let data = make_mozlz4("{not valid json");
        std::fs::write(tmp.path().join("zen-sessions.jsonlz4"), &data).unwrap();
        let err = parse_zen_workspaces(tmp.path()).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)), "got {:?}", err);
    }

    #[test]
    fn parses_minimal_workspaces() {
        let tmp = tempfile::tempdir().unwrap();

        let json = serde_json::json!({
            "spaces": [
                {
                    "uuid": "ws-1",
                    "name": "Work",
                    "userContextId": 1,
                    "icon": "💼",
                    "theme": {
                        "gradientColors": [{"color": "#ff0000"}]
                    }
                }
            ],
            "tabs": [
                {
                    "zenWorkspace": "ws-1",
                    "entries": [{"url": "https://example.com", "title": "Example"}],
                    "pinned": false,
                    "zenEssential": false
                },
                {
                    "zenWorkspace": "ws-1",
                    "entries": [{"url": "https://pinned.com", "title": "Pinned"}],
                    "pinned": true,
                    "zenEssential": false
                },
                {
                    "zenWorkspace": "ws-1",
                    "entries": [{"url": "https://essential.com", "title": "Essential"}],
                    "pinned": false,
                    "zenEssential": true
                }
            ],
            "folders": []
        });

        let data = make_mozlz4(&serde_json::to_string(&json).unwrap());
        std::fs::write(tmp.path().join("zen-sessions.jsonlz4"), &data).unwrap();

        // Write prefs.js for active workspace
        std::fs::write(
            tmp.path().join("prefs.js"),
            "user_pref(\"zen.workspaces.active\", \"ws-1\");\n",
        )
        .unwrap();

        let result = parse_zen_workspaces(tmp.path()).unwrap();
        assert_eq!(result.workspaces.len(), 1);
        assert_eq!(result.active_workspace_uuid, "ws-1");

        let ws = &result.workspaces[0];
        assert_eq!(ws.uuid, "ws-1");
        assert_eq!(ws.name, "Work");
        assert_eq!(ws.icon, "💼");
        assert_eq!(ws.theme_color_hex, "#ff0000");
        assert_eq!(ws.container_id, 1);
        assert_eq!(ws.regular_tabs.len(), 1);
        assert_eq!(ws.regular_tabs[0].url, "https://example.com");
        assert_eq!(ws.pinned_tabs.len(), 1);
        assert_eq!(ws.pinned_tabs[0].url, "https://pinned.com");
        assert_eq!(ws.favorites.len(), 1);
        assert_eq!(ws.favorites[0].url, "https://essential.com");
    }

    #[test]
    fn tabs_in_folders() {
        let tmp = tempfile::tempdir().unwrap();

        let json = serde_json::json!({
            "spaces": [{"uuid": "ws-1", "name": "Main", "userContextId": 1}],
            "tabs": [
                {
                    "zenWorkspace": "ws-1",
                    "entries": [{"url": "https://foldered.com", "title": "In Folder"}],
                    "folderId": "f-1",
                    "pinned": false,
                    "zenEssential": false
                }
            ],
            "folders": [
                {"id": "f-1", "name": "MyFolder", "workspaceId": "ws-1"}
            ]
        });

        let data = make_mozlz4(&serde_json::to_string(&json).unwrap());
        std::fs::write(tmp.path().join("zen-sessions.jsonlz4"), &data).unwrap();

        let result = parse_zen_workspaces(tmp.path()).unwrap();
        let ws = &result.workspaces[0];
        assert_eq!(ws.folders.len(), 1);
        assert_eq!(ws.folders[0].name, "MyFolder");
        assert_eq!(ws.folders[0].tabs.len(), 1);
        assert_eq!(ws.folders[0].tabs[0].url, "https://foldered.com");
        assert!(ws.regular_tabs.is_empty());
    }

    #[test]
    fn skips_about_blank_tabs() {
        let tmp = tempfile::tempdir().unwrap();

        let json = serde_json::json!({
            "spaces": [{"uuid": "ws-1", "name": "S", "userContextId": 1}],
            "tabs": [
                {
                    "zenWorkspace": "ws-1",
                    "entries": [{"url": "about:blank", "title": ""}],
                    "pinned": false,
                    "zenEssential": false
                }
            ],
            "folders": []
        });

        let data = make_mozlz4(&serde_json::to_string(&json).unwrap());
        std::fs::write(tmp.path().join("zen-sessions.jsonlz4"), &data).unwrap();

        let result = parse_zen_workspaces(tmp.path()).unwrap();
        assert!(result.workspaces[0].regular_tabs.is_empty());
    }

    #[test]
    fn rejects_cyclic_folders() {
        let tmp = tempfile::tempdir().unwrap();

        let json = serde_json::json!({
            "spaces": [{"uuid": "ws-1", "name": "S", "userContextId": 1}],
            "tabs": [],
            "folders": [
                {"id": "f-1", "name": "A", "workspaceId": "ws-1", "parentId": "f-2"},
                {"id": "f-2", "name": "B", "workspaceId": "ws-1", "parentId": "f-1"}
            ]
        });

        let data = make_mozlz4(&serde_json::to_string(&json).unwrap());
        std::fs::write(tmp.path().join("zen-sessions.jsonlz4"), &data).unwrap();

        let result = parse_zen_workspaces(tmp.path()).unwrap();
        assert!(result.workspaces[0].folders.is_empty());
    }

    #[test]
    fn preserves_up_to_5_level_folder_nesting_and_rejects_deeper() {
        let tmp = tempfile::tempdir().unwrap();

        let json = serde_json::json!({
            "spaces": [{"uuid": "ws-1", "name": "S", "userContextId": 1}],
            "tabs": [],
            "folders": [
                {"id": "l0", "name": "Root",  "workspaceId": "ws-1", "parentId": ""},
                {"id": "l1", "name": "L1",    "workspaceId": "ws-1", "parentId": "l0"},
                {"id": "l2", "name": "L2",    "workspaceId": "ws-1", "parentId": "l1"},
                {"id": "l3", "name": "L3",    "workspaceId": "ws-1", "parentId": "l2"},
                {"id": "l4", "name": "L4",    "workspaceId": "ws-1", "parentId": "l3"},
                {"id": "l5", "name": "TooDeep","workspaceId": "ws-1", "parentId": "l4"}
            ]
        });

        let data = make_mozlz4(&serde_json::to_string(&json).unwrap());
        std::fs::write(tmp.path().join("zen-sessions.jsonlz4"), &data).unwrap();

        let result = parse_zen_workspaces(tmp.path()).unwrap();
        let names: Vec<_> = result.workspaces[0]
            .folders
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(names.contains(&"Root"));
        assert!(names.contains(&"L1"));
        assert!(names.contains(&"L2"));
        assert!(names.contains(&"L3"));
        assert!(names.contains(&"L4"));
        assert!(
            !names.contains(&"TooDeep"),
            "folders nested >5 ancestors deep must be rejected, got {:?}",
            names
        );
        assert_eq!(result.workspaces[0].folders.len(), 5);
    }
}
