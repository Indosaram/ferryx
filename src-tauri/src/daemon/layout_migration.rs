//! One-way import of frontend-owned session layouts. Visual identities only leave
//! this module in the ephemeral mapping returned by the first LayoutGet.
use super::layout::{Content, Direction, Layout, Node, Tab, TabGroupLayout};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn text<'a>(value: &'a Value, key: &str) -> &'a str { value[key].as_str().unwrap_or("") }
fn direction(value: &Value) -> Direction {
    if text(value, "direction") == "vertical" { Direction::Vertical } else { Direction::Horizontal }
}

pub fn migrate(primary: &Path, window: &str) -> Result<(Layout, BTreeMap<String, u64>), String> {
    let source = crate::session::session_path_for_window(primary, window)
        .map_err(|error| error.to_string())?;
    let backup = source.with_file_name(format!("{}.pre-daemon-layout.json", source.file_stem().ok_or("Session file name missing")?.to_string_lossy()));
    let bytes = match std::fs::read(&backup) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match std::fs::read(&source) {
            Ok(bytes) => {
                // Never replace an earlier migration snapshot or remove the legacy file.
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&backup).map_err(|error| error.to_string())?;
                file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|error| error.to_string())?;
                bytes
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((Layout::default(), BTreeMap::new())),
            Err(error) => return Err(error.to_string()),
        },
        Err(error) => return Err(error.to_string()),
    };
    let session: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    convert(&session)
}

fn convert(session: &Value) -> Result<(Layout, BTreeMap<String, u64>), String> {
    let mut mapping = BTreeMap::new();
    let mut layouts = BTreeMap::new();
    let mut active_key = None;
    if let Some(workspaces) = session["workspaces"].as_object() {
        for (id, workspace) in workspaces {
            let key = scope_key(id, None);
            if id == text(session, "activeWorkspaceId") || active_key.is_none() { active_key = Some(key.clone()); }
            layouts.insert(key, convert_workspace(workspace, id, &mut mapping)?);
        }
    } else {
        let key = scope_key("", None);
        active_key = Some(key.clone());
        layouts.insert(key, convert_workspace(session, "", &mut mapping)?);
    }
    let mut layout = active_key.and_then(|key| layouts.remove(&key)).unwrap_or_default();
    layout.parked_layouts.extend(layouts);
    Ok((layout, mapping))
}

// JSON tuples remain unambiguous when paths and historical ids contain separators.
fn scope_key(workspace: &str, worktree: Option<&str>) -> String {
    serde_json::json!([workspace, worktree]).to_string()
}
fn identity_key(scope: &str, tab: &str, visual: &str) -> String {
    serde_json::json!([scope, tab, visual]).to_string()
}

fn convert_workspace(workspace: &Value, id: &str, mapping: &mut BTreeMap<String, u64>) -> Result<Layout, String> {
    let saved = workspace.get("layout").unwrap_or(workspace);
    let active_path = workspace["activeWorktreePath"].as_str().filter(|path| !path.is_empty());
    let mut layout = convert_saved(saved, workspace, &scope_key(id, active_path), mapping)?;
    for field in ["layoutByWorktree", "worktreeLayouts"] {
        if let Some(parked) = workspace[field].as_object() {
            for (path, saved) in parked {
                let key = scope_key(id, Some(path));
                let imported = convert_saved(saved, workspace, &key, mapping)?;
                layout.parked_layouts.insert(key, imported);
            }
        }
    }
    Ok(layout)
}

fn convert_saved(saved: &Value, workspace: &Value, scope: &str, mapping: &mut BTreeMap<String, u64>) -> Result<Layout, String> {
    let mut layout = Layout::default();
    layout.scope = Some(scope.into());
    if let Some(tabs) = saved["tabs"].as_array() {
        for tab in tabs {
            let id = text(tab, "id").to_owned();
            if id.is_empty() || layout.tabs.iter().any(|other| other.tab_id == id) { return Err("Invalid legacy tab identity".into()); }
            let state = saved["layoutsByTabId"].get(&id).or_else(|| tab.get("terminal")).unwrap_or(tab);
            let tree = state.get("root").or_else(|| state.get("paneTree"));
            let mut tab_mapping = BTreeMap::new();
            let pane_tree = match tree {
                Some(tree) => convert_node(tree, state, workspace, tab, &mut layout, &mut tab_mapping)?,
                None => { let content = content(state, workspace, tab, "")?; layout.mint(content) },
            };
            let active = tab_mapping.get(text(state, "activeLeafId")).copied().filter(|id| contains(&pane_tree, *id)).unwrap_or_else(|| first(&pane_tree));
            let expanded = tab_mapping.get(text(state, "expandedLeafId")).copied().filter(|id| contains(&pane_tree, *id));
            mapping.extend(tab_mapping.into_iter().map(|(visual, node)| (identity_key(scope, &id, &visual), node)));
            layout.tabs.push(Tab { tab_id: id, label: text(tab, "label").into(), pinned: tab["pinned"].as_bool().unwrap_or(false), pane_tree, active_node_id: active, expanded_node_id: expanded });
        }
    }
    layout.active_tab_id = saved["activeTabId"].as_str().map(String::from).or_else(|| layout.tabs.first().map(|tab| tab.tab_id.clone()));
    layout.tab_group_layout = if saved["tabGroupLayout"].is_object() {
        convert_group(&saved["tabGroupLayout"], &saved["tabGroups"])?
    } else {
        TabGroupLayout::Group { group_id: "main".into(), tab_ids: layout.tabs.iter().map(|tab| tab.tab_id.clone()).collect(), active_tab_id: layout.active_tab_id.clone() }
    };
    layout.focused_group_id = saved["focusedGroupId"].as_str().unwrap_or("main").into();
    Ok(layout)
}

fn first(node: &Node) -> u64 { match node { Node::Leaf { node_id, .. } => *node_id, Node::Split { a, .. } => first(a) } }
fn contains(node: &Node, id: u64) -> bool { match node { Node::Leaf { node_id, .. } => *node_id == id, Node::Split { a, b, .. } => contains(a, id) || contains(b, id) } }

fn convert_node(tree: &Value, state: &Value, workspace: &Value, tab: &Value, layout: &mut Layout, mapping: &mut BTreeMap<String, u64>) -> Result<Node, String> {
    if text(tree, "type") == "split" {
        return Ok(Node::Split { dir: direction(tree), ratio: tree["ratio"].as_f64().unwrap_or(0.5).clamp(0.1, 0.9),
            a: Box::new(convert_node(&tree["first"], state, workspace, tab, layout, mapping)?),
            b: Box::new(convert_node(&tree["second"], state, workspace, tab, layout, mapping)?),
        });
    }
    let visual = text(tree, "leafId");
    let node = layout.mint(content(state, workspace, tab, visual)?);
    if !visual.is_empty() { mapping.insert(visual.into(), first(&node)); }
    Ok(node)
}

fn content(state: &Value, workspace: &Value, tab: &Value, visual: &str) -> Result<Content, String> {
    let pane = &state["contentsByLeafId"][visual];
    let kind = pane["kind"].as_str().or_else(|| tab["kind"].as_str()).unwrap_or("terminal");
    Ok(match kind {
        "file" => {
            let fallback = tab.get("file").unwrap_or(tab);
            let file = if pane.is_null() { fallback } else { pane.get("file").unwrap_or(pane) };
            Content::File { path: file["path"].as_str().or_else(|| fallback["path"].as_str()).unwrap_or("").into(), line: file["line"].as_u64(), col: file["col"].as_u64(), ownership: metadata(file, fallback, &["backendSessionId", "workspaceId", "previewId", "remote"]) }
        },
        "browser" => Content::Browser { url: pane["browser"]["url"].as_str().or_else(|| pane["url"].as_str()).or_else(|| tab["browser"]["url"].as_str()).or_else(|| tab["url"].as_str()).unwrap_or("").into(), identity: metadata(pane.get("browser").unwrap_or(pane), tab.get("browser").unwrap_or(tab), &["browserId", "workspaceId", "worktreePath", "profileId"]) },
        "dag" => Content::Dag,
        "changes" => Content::Changes { checkout: metadata(pane, tab.get("changes").unwrap_or(tab), &["workspaceId", "worktreePath", "worktree"]) },
        "remote-desktop" | "remoteDesktop" => Content::RemoteDesktop { host_id: text(pane, "hostId").into() },
        _ => {
            let frontend = pane["sessionId"].as_str().or_else(|| state["sessionIdsByLeafId"][visual].as_str())
                .or_else(|| state["primarySessionId"].as_str()).or_else(|| tab["sessionId"].as_str()).unwrap_or("");
            let backend = workspace["terminalSessions"][frontend]["backendSessionId"].as_str()
                .or_else(|| pane["backendSessionId"].as_str()).unwrap_or("");
            Content::Terminal { backend_session_id: backend.into() }
        },
    })
}

fn metadata(pane: &Value, tab: &Value, fields: &[&str]) -> BTreeMap<String, Value> {
    fields.iter().filter_map(|field| pane.get(*field).or_else(|| tab.get(*field)).map(|value| ((*field).into(), value.clone()))).collect()
}

fn convert_group(tree: &Value, groups: &Value) -> Result<TabGroupLayout, String> {
    if text(tree, "type") == "split" {
        return Ok(TabGroupLayout::Split { dir: direction(tree), ratio: tree["ratio"].as_f64().unwrap_or(0.5).clamp(0.1, 0.9),
            a: Box::new(convert_group(&tree["first"], groups)?), b: Box::new(convert_group(&tree["second"], groups)?),
        });
    }
    let id = text(tree, "groupId");
    let group = groups.get(id).or_else(|| groups.as_array().and_then(|groups| groups.iter().find(|group| text(group, "id") == id))).ok_or("Missing legacy tab group")?;
    Ok(TabGroupLayout::Group { group_id: id.into(), tab_ids: group["tabIds"].as_array().map(|ids| ids.iter().filter_map(|id| id.as_str().map(String::from)).collect()).unwrap_or_default(), active_tab_id: group["activeTabId"].as_str().map(String::from) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::layout::Store;

    fn repeated_ids() -> Value {
        serde_json::json!({"tabs":[{"id":"one"},{"id":"two"}],"layoutsByTabId":{
            "one":{"root":{"type":"leaf","leafId":"repeated"},"activeLeafId":"repeated","expandedLeafId":"repeated"},
            "two":{"root":{"type":"leaf","leafId":"repeated"},"activeLeafId":"repeated","expandedLeafId":"repeated"}
        }})
    }

    #[test]
    fn historical_visual_ids_are_scoped_by_workspace_worktree_and_tab() {
        let session = serde_json::json!({"activeWorkspaceId":"w","workspaces":{
            "w":{"layout":repeated_ids(),"worktreeLayouts":{"/parked":repeated_ids()}},
            "other":{"layout":repeated_ids()}
        }});
        let (layout, mapping) = convert(&session).unwrap();
        assert_eq!(mapping.len(), 6);
        for (scope, saved) in [
            (scope_key("w", None), &layout),
            (scope_key("w", Some("/parked")), &layout.parked_layouts[&scope_key("w", Some("/parked"))]),
            (scope_key("other", None), &layout.parked_layouts[&scope_key("other", None)]),
        ] {
            for (index, tab) in saved.tabs.iter().enumerate() {
                let node = index as u64 + 1;
                assert_eq!(mapping[&identity_key(&scope, &tab.tab_id, "repeated")], node);
                assert_eq!(tab.active_node_id, node);
                assert_eq!(tab.expanded_node_id, Some(node));
            }
        }
        assert_ne!(identity_key("a", "b/c", "d"), identity_key("a", "b", "c/d"));
    }

    #[test]
    fn parked_workspace_and_worktree_layouts_survive_migration_and_restart() {
        for field in ["worktreeLayouts", "layoutByWorktree"] {
            let root = tempfile::tempdir().unwrap();
            let mut workspace = serde_json::json!({"layout":repeated_ids()});
            workspace[field] = serde_json::json!({"/parked":repeated_ids()});
            let session = serde_json::json!({"activeWorkspaceId":"w","workspaces":{
                "w":workspace,"other":{"layout":repeated_ids(),"worktreeLayouts":{"/other":repeated_ids()}}
            }});
            std::fs::write(root.path().join("session_state.json"), serde_json::to_vec(&session).unwrap()).unwrap();
            let view = Store::new(root.path().into()).get("main").unwrap();
            assert_eq!(view.layout.parked_layouts.len(), 2);
            let other = &view.layout.parked_layouts[&scope_key("other", None)];
            assert_eq!(other.parked_layouts[&scope_key("other", Some("/other"))].tabs.len(), 2);
            assert_eq!(view.layout.parked_layouts[&scope_key("w", Some("/parked"))].tabs.len(), 2);
            assert_eq!(Store::new(root.path().into()).get("main").unwrap().layout, view.layout);
            let mut successor = Store::new(root.path().join("successor"));
            successor.adopt(BTreeMap::from([("main".into(), view.layout.clone())])).unwrap();
            assert_eq!(successor.get("main").unwrap().layout, view.layout);
        }
    }

    #[test]
    fn three_saved_session_shapes_and_corruption_recovery() {
        let fixtures = [
            serde_json::json!({"tabs":[{"id":"t","label":"old","sessionId":"s","paneTree":{"type":"leaf","leafId":"visual"},"sessionIdsByLeafId":{"visual":"s"}}]}),
            serde_json::json!({"tabs":[{"id":"t","label":"terminal","terminal":{"primarySessionId":"s","paneTree":{"type":"leaf","leafId":"visual"},"sessionIdsByLeafId":{"visual":"s"}}}]}),
            serde_json::json!({"tabs":[{"id":"t","label":"mixed"}],"activeTabId":"t","layoutsByTabId":{"t":{"root":{"type":"split","direction":"vertical","ratio":0.4,"first":{"type":"leaf","leafId":"visual"},"second":{"type":"leaf","leafId":"browser"}},"activeLeafId":"visual","contentsByLeafId":{"visual":{"kind":"terminal","sessionId":"s"},"browser":{"kind":"browser","browser":{"url":"https://example.org"}}}}},"tabGroups":[{"id":"g","tabIds":["t"],"activeTabId":"t"}],"tabGroupLayout":{"type":"group","groupId":"g"},"focusedGroupId":"g"}),
        ];
        for saved in fixtures {
            let root = tempfile::tempdir().unwrap();
            let session = serde_json::json!({"version":3,"activeWorkspaceId":"w","workspaces":{"w":{"layout":saved,"terminalSessions":{"s":{"backendSessionId":"pty-stable"}}}}});
            let bytes = serde_json::to_vec(&session).unwrap();
            let source = root.path().join("session_state.json");
            std::fs::write(&source, &bytes).unwrap();
            let mut store = Store::new(root.path().into());
            let view = store.get("main").unwrap();
            assert_eq!(view.migration_map[&identity_key(&scope_key("w", None), "t", "visual")], 1);
            assert!(store.get("main").unwrap().migration_map.is_empty());
            let persisted = std::fs::read_to_string(root.path().join("layout/main.json")).unwrap();
            assert!(!persisted.contains("visual"));
            assert!(!persisted.contains("leafId"));
            assert!(persisted.contains("pty-stable"));
            assert_eq!(std::fs::read(&source).unwrap(), bytes);
            assert_eq!(std::fs::read(root.path().join("session_state.pre-daemon-layout.json")).unwrap(), bytes);
            std::fs::write(root.path().join("layout/main.json"), b"{corrupt").unwrap();
            std::fs::write(&source, b"also corrupt").unwrap();
            assert_eq!(Store::new(root.path().into()).get("main").unwrap().layout, view.layout);
        }
    }
}
