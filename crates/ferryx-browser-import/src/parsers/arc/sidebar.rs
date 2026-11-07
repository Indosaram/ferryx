//! Arc browser sidebar.plist / StorableSidebar.json parser.
//!
//! Ports `maho-chromium/browser/importer/arc_sidebar_parser.cc` to Rust.
//! Reads `<arc_user_data_dir>/StorableSidebar.json` and extracts spaces +
//! tabs + folders into `ParsedWorkspace` structs.
//!
//! Arc stores its sidebar tree as alternating `[uuid, obj, uuid, obj, ...]`
//! arrays. The parser walks two possible roots:
//! - Path B (preferred): `firebaseSyncState.syncData.{spaceModels,items}`
//! - Path A (fallback):  `sidebar.containers[1].{spaces,items}`

use serde_json::Value;
use std::path::Path;

use crate::{ImportError, ImportResult, ParsedFolder, ParsedTab, ParsedWorkspace};

#[derive(Debug, Default)]
pub struct ArcParseResult {
    pub workspaces: Vec<ParsedWorkspace>,
    pub active_workspace_uuid: String,
}

fn resolve_sidebar_path(start: &Path) -> ImportResult<std::path::PathBuf> {
    let mut candidate = Some(start.to_path_buf());
    let mut tried: Vec<String> = Vec::new();
    for _ in 0..4 {
        let Some(dir) = candidate else { break };
        let file = dir.join("StorableSidebar.json");
        if file.exists() {
            return Ok(file);
        }
        tried.push(file.display().to_string());
        candidate = dir.parent().map(|p| p.to_path_buf());
    }
    Err(ImportError::FileNotFound(tried.join(" | ")))
}

pub fn parse_arc_sidebar(arc_user_data_dir: &Path) -> ImportResult<ArcParseResult> {
    let sidebar_path = resolve_sidebar_path(arc_user_data_dir)?;

    let content = std::fs::read_to_string(&sidebar_path)
        .map_err(|e| ImportError::Io(format!("{}: {}", sidebar_path.display(), e)))?;

    if content.is_empty() {
        return Ok(ArcParseResult::default());
    }

    let root: Value = serde_json::from_str(&content)
        .map_err(|e| ImportError::Parse(format!("StorableSidebar.json: {}", e)))?;

    let root_obj = root
        .as_object()
        .ok_or_else(|| ImportError::Parse("root is not a JSON object".into()))?;

    // Path B (preferred): firebaseSyncState.syncData
    let sync_data = root_obj
        .get("firebaseSyncState")
        .and_then(|v| v.as_object())
        .and_then(|s| s.get("syncData"))
        .and_then(|v| v.as_object());

    let (space_models, items_list) = if let Some(sd) = sync_data {
        (
            sd.get("spaceModels").and_then(|v| v.as_array()),
            sd.get("items").and_then(|v| v.as_array()),
        )
    } else {
        // Path A (fallback): sidebar.containers[1]
        let containers = root_obj
            .get("sidebar")
            .and_then(|v| v.as_object())
            .and_then(|s| s.get("containers"))
            .and_then(|v| v.as_array());

        if let Some(containers) = containers {
            if containers.len() > 1 {
                if let Some(container) = containers[1].as_object() {
                    (
                        container.get("spaces").and_then(|v| v.as_array()),
                        container.get("items").and_then(|v| v.as_array()),
                    )
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    };

    let (Some(space_models), Some(items_list)) = (space_models, items_list) else {
        return Ok(ArcParseResult::default());
    };

    let all_items = parse_alternating_array(items_list);
    let mut spaces = parse_alternating_spaces(space_models);

    // Read StorableWindows.json if present to detect the active focused space
    let windows_path = sidebar_path.parent().map(|p| p.join("StorableWindows.json"));
    let mut last_focused_space_id: Option<String> = None;
    if let Some(ref win_file) = windows_path {
        if win_file.exists() {
            if let Ok(win_content) = std::fs::read_to_string(win_file) {
                if let Ok(win_json) = serde_json::from_str::<Value>(&win_content) {
                    if let Some(focused) = win_json.get("lastFocusedSpaceID").and_then(|v| v.as_str()) {
                        last_focused_space_id = Some(focused.to_string());
                    }
                }
            }
        }
    }

    // Arc syncData stores spaceModels in reverse chronological order (newest space at the end).
    // If the last space matches lastFocusedSpaceID, reverse spaces to place the active/primary space first.
    if let Some(ref focused_id) = last_focused_space_id {
        if let Some(pos) = spaces.iter().position(|(id, _)| id == focused_id) {
            if pos > 0 && pos == spaces.len() - 1 {
                spaces.reverse();
            }
        }
    }

    let mut result = ArcParseResult::default();

    for (space_id, space_dict) in spaces {
        let mut ws = ParsedWorkspace {
            name: space_dict
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Space")
                .to_string(),
            uuid: if !space_id.is_empty() {
                space_id
            } else {
                result.workspaces.len().to_string()
            },
            ..Default::default()
        };

        if let Some(custom_info) = space_dict.get("customInfo").and_then(|v| v.as_object()) {
            if let Some(icon_type) = custom_info.get("iconType").and_then(|v| v.as_object()) {
                if let Some(emoji) = icon_type.get("emoji_v2").and_then(|v| v.as_str()) {
                    ws.icon = emoji.to_string();
                } else if let Some(name) = icon_type.get("icon").and_then(|v| v.as_str()) {
                    ws.icon = name.to_string();
                }
            }
        }

        // containerIDs alternate: [type, uuid, type, uuid, ...]
        if let Some(container_ids) = space_dict.get("containerIDs").and_then(|v| v.as_array()) {
            let mut i = 0;
            while i + 1 < container_ids.len() {
                let type_val = container_ids[i].as_str();
                let uuid_val = container_ids[i + 1].as_str();
                i += 2;

                let (Some(container_type), Some(target_uuid)) = (type_val, uuid_val) else {
                    continue;
                };
                let is_pinned = container_type == "pinned";

                if let Some(item) = find_item_by_id(&all_items, target_uuid) {
                    if let Some(children) = find_children_ids(item) {
                        parse_items_into_workspace(
                            &mut ws,
                            &all_items,
                            children,
                            is_pinned,
                            None,
                            &mut std::collections::HashSet::new(),
                        );
                    }
                }
            }
        }

        result.workspaces.push(ws);
    }

    // Resolve active workspace UUID
    if let Some(ref focused_id) = last_focused_space_id {
        if result.workspaces.iter().any(|w| &w.uuid == focused_id) {
            result.active_workspace_uuid = focused_id.clone();
        }
    }
    if result.active_workspace_uuid.is_empty() && !result.workspaces.is_empty() {
        result.active_workspace_uuid = result.workspaces[0].uuid.clone();
    }

    // Extract global favorites from topAppsContainerIDs if present
    let mut top_apps_id: Option<String> = None;
    if let Some(containers) = root_obj
        .get("sidebar")
        .and_then(|v| v.as_object())
        .and_then(|s| s.get("containers"))
        .and_then(|v| v.as_array())
    {
        if containers.len() > 1 {
            if let Some(c1) = containers[1].as_object() {
                if let Some(arr) = c1.get("topAppsContainerIDs").and_then(|v| v.as_array()) {
                    for v in arr {
                        if let Some(id_str) = v.as_str() {
                            top_apps_id = Some(id_str.to_string());
                        }
                    }
                }
            }
        }
    }

    if let Some(ref tid) = top_apps_id {
        if let Some(top_item) = find_item_by_id(&all_items, tid) {
            if let Some(children) = find_children_ids(top_item) {
                let mut fav_tabs = Vec::new();
                for child in children {
                    let Some(child_id) = child.as_str() else { continue; };
                    if let Some(tab_item) = find_item_by_id(&all_items, child_id) {
                        if is_tab_item(tab_item) {
                            let url = extract_tab_url(tab_item);
                            if !url.is_empty() {
                                if let Ok(parsed) = url::Url::parse(&url) {
                                    let scheme = parsed.scheme();
                                    if scheme == "http" || scheme == "https" {
                                        fav_tabs.push(ParsedTab {
                                            url,
                                            title: extract_tab_title(tab_item),
                                            is_pinned: true,
                                            is_essential: false,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                if !fav_tabs.is_empty() && !result.workspaces.is_empty() {
                    let target_idx = result
                        .workspaces
                        .iter()
                        .position(|w| w.uuid == result.active_workspace_uuid)
                        .unwrap_or(0);
                    result.workspaces[target_idx].favorites = fav_tabs;
                }
            }
        }
    }

    Ok(result)
}

/// Strip Arc's `{ "value": <inner> }` wrapper if present, otherwise return as-is.
fn unwrap_value(val: &Value) -> Option<&serde_json::Map<String, Value>> {
    let dict = val.as_object()?;
    if let Some(inner) = dict.get("value").and_then(|v| v.as_object()) {
        Some(inner)
    } else {
        Some(dict)
    }
}

/// Arc alternating arrays: `[uuid, obj, uuid, obj, ...]`. We only want the obj at odd indices.
fn parse_alternating_array(list: &[Value]) -> Vec<&serde_json::Map<String, Value>> {
    let mut result = Vec::with_capacity(list.len() / 2);
    let mut i = 1;
    while i < list.len() {
        if let Some(unwrapped) = unwrap_value(&list[i]) {
            result.push(unwrapped);
        }
        i += 2;
    }
    result
}

/// Arc alternating space arrays: `[uuid, obj, uuid, obj, ...]`. Retains (space_id, space_obj).
fn parse_alternating_spaces(list: &[Value]) -> Vec<(String, &serde_json::Map<String, Value>)> {
    let mut result = Vec::with_capacity(list.len() / 2);
    let mut i = 0;
    while i + 1 < list.len() {
        let id_str = list[i].as_str().unwrap_or("").to_string();
        if let Some(unwrapped) = unwrap_value(&list[i + 1]) {
            result.push((id_str, unwrapped));
        }
        i += 2;
    }
    result
}

fn find_children_ids(item: &serde_json::Map<String, Value>) -> Option<&Vec<Value>> {
    if let Some(direct) = item.get("childrenIds").and_then(|v| v.as_array()) {
        return Some(direct);
    }
    let wrapped = item.get("childrenIds").and_then(|v| v.as_object())?;
    if let Some(inner) = wrapped.get("_0").and_then(|v| v.as_array()) {
        return Some(inner);
    }
    wrapped.get("items").and_then(|v| v.as_array())
}

fn is_tab_item(item: &serde_json::Map<String, Value>) -> bool {
    item.get("data")
        .and_then(|v| v.as_object())
        .and_then(|d| d.get("tab"))
        .and_then(|v| v.as_object())
        .is_some()
}

/// A "real" user-visible folder has `data.list`. Items with `data.itemContainer`
/// are space-level root containers (pinned/unpinned roots), not user folders —
/// they have null titles and exist as structural roots only. Treating them as
/// folders pollutes LMDB with empty-named placeholder folders.
fn is_folder_item(item: &serde_json::Map<String, Value>) -> bool {
    item.get("data")
        .and_then(|v| v.as_object())
        .and_then(|d| d.get("list"))
        .is_some()
}

fn extract_tab_url(item: &serde_json::Map<String, Value>) -> String {
    item.get("data")
        .and_then(|v| v.as_object())
        .and_then(|d| d.get("tab"))
        .and_then(|v| v.as_object())
        .and_then(|t| t.get("savedURL"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_tab_title(item: &serde_json::Map<String, Value>) -> String {
    item.get("data")
        .and_then(|v| v.as_object())
        .and_then(|d| d.get("tab"))
        .and_then(|v| v.as_object())
        .and_then(|t| t.get("savedTitle"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_item_title(item: &serde_json::Map<String, Value>) -> String {
    let raw = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if raw.is_empty() {
        "Untitled".to_string()
    } else {
        raw.to_string()
    }
}

fn find_item_by_id<'a>(
    all_items: &[&'a serde_json::Map<String, Value>],
    target_id: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    all_items
        .iter()
        .find(|item| {
            item.get("id")
                .and_then(|v| v.as_str())
                .map(|id| id == target_id)
                .unwrap_or(false)
        })
        .copied()
}

fn parse_items_into_workspace(
    ws: &mut ParsedWorkspace,
    all_items: &[&serde_json::Map<String, Value>],
    container_ids: &[Value],
    is_pinned_section: bool,
    current_folder_id: Option<&str>,
    visited: &mut std::collections::HashSet<String>,
) {
    for child in container_ids {
        let Some(item_id) = child.as_str() else {
            continue;
        };

        if !visited.insert(item_id.to_string()) {
            continue;
        }

        let Some(item) = find_item_by_id(all_items, item_id) else {
            continue;
        };

        if is_tab_item(item) {
            let url = extract_tab_url(item);
            if url.is_empty() {
                continue;
            }
            let Ok(parsed) = url::Url::parse(&url) else {
                continue;
            };
            let scheme = parsed.scheme();
            if scheme != "http" && scheme != "https" {
                continue;
            }
            let tab = ParsedTab {
                url,
                title: extract_tab_title(item),
                is_pinned: is_pinned_section,
                is_essential: false,
            };
            match current_folder_id {
                Some(fid) => {
                    if let Some(folder) = ws.folders.iter_mut().find(|f| f.id == fid) {
                        folder.tabs.push(tab);
                    }
                }
                None => {
                    if is_pinned_section {
                        ws.pinned_tabs.push(tab);
                    } else {
                        ws.regular_tabs.push(tab);
                    }
                }
            }
        } else if is_folder_item(item) {
            let folder_id = item_id.to_string();
            ws.folders.push(ParsedFolder {
                id: folder_id.clone(),
                name: extract_item_title(item),
                parent_id: current_folder_id.map(String::from).unwrap_or_default(),
                workspace_id: ws.uuid.clone(),
                tabs: Vec::new(),
            });

            if let Some(children) = find_children_ids(item) {
                parse_items_into_workspace(
                    ws,
                    all_items,
                    children,
                    is_pinned_section,
                    Some(&folder_id),
                    visited,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = std::env::temp_dir().join("ferryx-import-test-no-arc");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let err = parse_arc_sidebar(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {:?}", err);
    }

    #[test]
    fn empty_file_returns_empty_result() {
        let tmp = std::env::temp_dir().join("ferryx-import-test-arc-empty");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("StorableSidebar.json"), "").unwrap();
        let res = parse_arc_sidebar(&tmp).unwrap();
        assert!(res.workspaces.is_empty());
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let tmp = std::env::temp_dir().join("ferryx-import-test-arc-bad");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("StorableSidebar.json"), "{not valid json").unwrap();
        let err = parse_arc_sidebar(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)), "got {:?}", err);
    }

    #[test]
    fn parses_minimal_path_a_fixture() {
        // Minimal sidebar.containers[1] structure with one space + one pinned tab.
        let fixture = serde_json::json!({
            "sidebar": {
                "containers": [
                    {},
                    {
                        "spaces": [
                            "uuid-skip",
                            {
                                "title": "Work",
                                "containerIDs": [
                                    "pinned", "container-1"
                                ],
                                "customInfo": {
                                    "iconType": { "emoji_v2": "🚀" }
                                }
                            }
                        ],
                        "items": [
                            "skip",
                            {
                                "id": "container-1",
                                "data": { "list": {} },
                                "childrenIds": ["skip", "tab-1"]
                            },
                            "skip",
                            {
                                "id": "tab-1",
                                "data": {
                                    "tab": {
                                        "savedURL": "https://example.com",
                                        "savedTitle": "Example"
                                    }
                                }
                            }
                        ]
                    }
                ]
            }
        });
        let tmp = std::env::temp_dir().join("ferryx-import-test-arc-minimal");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("StorableSidebar.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let res = parse_arc_sidebar(&tmp).unwrap();
        assert_eq!(res.workspaces.len(), 1);
        let ws = &res.workspaces[0];
        assert_eq!(ws.name, "Work");
        assert_eq!(ws.icon, "🚀");
        assert_eq!(ws.pinned_tabs.len(), 1);
        assert_eq!(ws.pinned_tabs[0].url, "https://example.com");
        assert_eq!(ws.pinned_tabs[0].title, "Example");
        assert!(ws.pinned_tabs[0].is_pinned);
    }

    #[test]
    fn rejects_non_http_scheme() {
        let fixture = serde_json::json!({
            "sidebar": {
                "containers": [
                    {},
                    {
                        "spaces": [
                            "u",
                            {
                                "title": "S",
                                "containerIDs": ["pinned", "c1"]
                            }
                        ],
                        "items": [
                            "u",
                            {
                                "id": "c1",
                                "data": { "list": {} },
                                "childrenIds": ["u", "t1"]
                            },
                            "u",
                            {
                                "id": "t1",
                                "data": {
                                    "tab": { "savedURL": "javascript:alert(1)" }
                                }
                            }
                        ]
                    }
                ]
            }
        });
        let tmp = std::env::temp_dir().join("ferryx-import-test-arc-bad-scheme");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("StorableSidebar.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let res = parse_arc_sidebar(&tmp).unwrap();
        assert_eq!(res.workspaces.len(), 1);
        assert!(res.workspaces[0].pinned_tabs.is_empty());
    }

    fn write_fixture(name: &str, fixture: serde_json::Value) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("StorableSidebar.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();
        tmp
    }

    #[test]
    fn tabs_inside_folder_are_nested_not_flat() {
        let fixture = serde_json::json!({
            "sidebar": {
                "containers": [
                    {},
                    {
                        "spaces": [
                            "u",
                            { "title": "S", "containerIDs": ["pinned", "root"] }
                        ],
                        "items": [
                            "u",
                            { "id": "root", "data": { "list": {} }, "childrenIds": ["u", "folder-a", "u", "loose-tab"] },
                            "u",
                            { "id": "folder-a", "title": "Bookmarks",
                              "data": { "list": {} },
                              "childrenIds": ["u", "tab-in-folder"] },
                            "u",
                            { "id": "tab-in-folder",
                              "data": { "tab": { "savedURL": "https://nested.example.com", "savedTitle": "Nested" } } },
                            "u",
                            { "id": "loose-tab",
                              "data": { "tab": { "savedURL": "https://loose.example.com", "savedTitle": "Loose" } } }
                        ]
                    }
                ]
            }
        });
        let tmp = write_fixture("ferryx-import-test-arc-nested-tab", fixture);
        let res = parse_arc_sidebar(&tmp).unwrap();
        let ws = &res.workspaces[0];
        assert_eq!(ws.folders.len(), 1);
        assert_eq!(ws.folders[0].name, "Bookmarks");
        assert_eq!(ws.folders[0].tabs.len(), 1);
        assert_eq!(ws.folders[0].tabs[0].url, "https://nested.example.com");
        assert_eq!(ws.pinned_tabs.len(), 1);
        assert_eq!(ws.pinned_tabs[0].url, "https://loose.example.com");
    }

    #[test]
    fn nested_folder_has_parent_id() {
        let fixture = serde_json::json!({
            "sidebar": {
                "containers": [
                    {},
                    {
                        "spaces": [
                            "u",
                            { "title": "S", "containerIDs": ["pinned", "root"] }
                        ],
                        "items": [
                            "u", { "id": "root", "data": { "list": {} }, "childrenIds": ["u", "outer"] },
                            "u", { "id": "outer", "title": "Outer",
                                   "data": { "list": {} },
                                   "childrenIds": ["u", "inner"] },
                            "u", { "id": "inner", "title": "Inner",
                                   "data": { "list": {} },
                                   "childrenIds": [] }
                        ]
                    }
                ]
            }
        });
        let tmp = write_fixture("ferryx-import-test-arc-nested-folder", fixture);
        let res = parse_arc_sidebar(&tmp).unwrap();
        let ws = &res.workspaces[0];
        assert_eq!(ws.folders.len(), 2);
        let outer = ws.folders.iter().find(|f| f.name == "Outer").unwrap();
        let inner = ws.folders.iter().find(|f| f.name == "Inner").unwrap();
        assert!(outer.parent_id.is_empty(), "outer should be root-level");
        assert_eq!(
            inner.parent_id, outer.id,
            "inner parent_id should be outer's id"
        );
    }

    #[test]
    fn item_container_is_not_classified_as_folder() {
        let fixture = serde_json::json!({
            "sidebar": {
                "containers": [
                    {},
                    {
                        "spaces": [
                            "u",
                            { "title": "S", "containerIDs": ["pinned", "root"] }
                        ],
                        "items": [
                            "u",
                            { "id": "root", "data": { "list": {} }, "childrenIds": ["u", "structural"] },
                            "u",
                            { "id": "structural", "title": null,
                              "data": { "itemContainer": { "containerType": { "spaceItems": { "_0": "space-xyz" } } } },
                              "childrenIds": [] }
                        ]
                    }
                ]
            }
        });
        let tmp = write_fixture("ferryx-import-test-arc-itemcontainer", fixture);
        let res = parse_arc_sidebar(&tmp).unwrap();
        let ws = &res.workspaces[0];
        assert!(
            ws.folders.is_empty(),
            "itemContainer must not become a folder"
        );
    }

    #[test]
    fn folder_with_empty_title_falls_back_to_untitled() {
        let fixture = serde_json::json!({
            "sidebar": {
                "containers": [
                    {},
                    {
                        "spaces": [
                            "u",
                            { "title": "S", "containerIDs": ["pinned", "root"] }
                        ],
                        "items": [
                            "u", { "id": "root", "data": { "list": {} }, "childrenIds": ["u", "blank"] },
                            "u", { "id": "blank", "title": "   ", "data": { "list": {} }, "childrenIds": [] }
                        ]
                    }
                ]
            }
        });
        let tmp = write_fixture("ferryx-import-test-arc-blank-title", fixture);
        let res = parse_arc_sidebar(&tmp).unwrap();
        assert_eq!(res.workspaces[0].folders[0].name, "Untitled");
    }

    #[test]
    fn cycle_in_children_does_not_infinite_loop() {
        let fixture = serde_json::json!({
            "sidebar": {
                "containers": [
                    {},
                    {
                        "spaces": [
                            "u",
                            { "title": "S", "containerIDs": ["pinned", "root"] }
                        ],
                        "items": [
                            "u", { "id": "root", "data": { "list": {} }, "childrenIds": ["u", "a"] },
                            "u", { "id": "a", "title": "A",
                                   "data": { "list": {} },
                                   "childrenIds": ["u", "b"] },
                            "u", { "id": "b", "title": "B",
                                   "data": { "list": {} },
                                   "childrenIds": ["u", "a"] }
                        ]
                    }
                ]
            }
        });
        let tmp = write_fixture("ferryx-import-test-arc-cycle", fixture);
        let res = parse_arc_sidebar(&tmp).unwrap();
        assert_eq!(res.workspaces[0].folders.len(), 2);
    }

    #[test]
    fn arc_real_format_parses_flat_children_favorites_and_focused_space() {
        let fixture = serde_json::json!({
            "firebaseSyncState": {
                "syncData": {
                    "spaceModels": [
                        "space-sap",
                        {
                            "title": "SAP",
                            "containerIDs": ["pinned", "sap-pinned", "unpinned", "sap-unpinned"]
                        },
                        "space-porsche",
                        {
                            "title": "Porsche",
                            "containerIDs": ["pinned", "porsche-pinned", "unpinned", "porsche-unpinned"]
                        }
                    ],
                    "items": [
                        "top-apps-container",
                        {
                            "id": "top-apps-container",
                            "data": { "itemContainer": {} },
                            "childrenIds": ["fav-1", "fav-2"]
                        },
                        "fav-1",
                        {
                            "id": "fav-1",
                            "data": { "tab": { "savedURL": "https://notion.so", "savedTitle": "Notion" } }
                        },
                        "fav-2",
                        {
                            "id": "fav-2",
                            "data": { "tab": { "savedURL": "https://youtube.com", "savedTitle": "YouTube" } }
                        },
                        "porsche-pinned",
                        {
                            "id": "porsche-pinned",
                            "data": { "itemContainer": {} },
                            "childrenIds": ["pin-1", "pin-2"]
                        },
                        "pin-1",
                        {
                            "id": "pin-1",
                            "data": { "tab": { "savedURL": "https://github.com", "savedTitle": "GitHub" } }
                        },
                        "pin-2",
                        {
                            "id": "pin-2",
                            "data": { "tab": { "savedURL": "https://google.com", "savedTitle": "Google" } }
                        },
                        "porsche-unpinned",
                        {
                            "id": "porsche-unpinned",
                            "data": { "itemContainer": {} },
                            "childrenIds": ["unpin-1"]
                        },
                        "unpin-1",
                        {
                            "id": "unpin-1",
                            "data": { "tab": { "savedURL": "https://news.ycombinator.com", "savedTitle": "Hacker News" } }
                        },
                        "sap-pinned",
                        {
                            "id": "sap-pinned",
                            "data": { "itemContainer": {} },
                            "childrenIds": []
                        },
                        "sap-unpinned",
                        {
                            "id": "sap-unpinned",
                            "data": { "itemContainer": {} },
                            "childrenIds": []
                        }
                    ]
                }
            },
            "sidebar": {
                "containers": [
                    {},
                    {
                        "topAppsContainerIDs": ["top-apps-container"],
                        "spaces": [],
                        "items": []
                    }
                ]
            }
        });

        let tmp = write_fixture("ferryx-import-test-arc-real-format", fixture);
        // Also write StorableWindows.json pointing to space-porsche
        let win_fixture = serde_json::json!({
            "lastFocusedSpaceID": "space-porsche"
        });
        std::fs::write(
            tmp.join("StorableWindows.json"),
            serde_json::to_string(&win_fixture).unwrap(),
        ).unwrap();

        let res = parse_arc_sidebar(&tmp).unwrap();
        assert_eq!(res.workspaces.len(), 2);
        // Focused space (Porsche) is reversed to index 0
        assert_eq!(res.workspaces[0].name, "Porsche");
        assert_eq!(res.workspaces[0].uuid, "space-porsche");
        assert_eq!(res.active_workspace_uuid, "space-porsche");

        // Favorites are extracted into the active workspace
        assert_eq!(res.workspaces[0].favorites.len(), 2);
        assert_eq!(res.workspaces[0].favorites[0].title, "Notion");
        assert_eq!(res.workspaces[0].favorites[1].title, "YouTube");

        // Flat childrenIds: both pin-1 and pin-2 are parsed (neither was skipped)
        assert_eq!(res.workspaces[0].pinned_tabs.len(), 2);
        assert_eq!(res.workspaces[0].pinned_tabs[0].title, "GitHub");
        assert_eq!(res.workspaces[0].pinned_tabs[1].title, "Google");

        // Unpinned tab is parsed
        assert_eq!(res.workspaces[0].regular_tabs.len(), 1);
        assert_eq!(res.workspaces[0].regular_tabs[0].title, "Hacker News");
    }
}
