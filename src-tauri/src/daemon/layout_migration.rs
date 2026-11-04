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
    let workspace = session["workspaces"].get(text(session, "activeWorkspaceId"))
        .or_else(|| session["workspaces"].as_object().and_then(|map| map.values().next()))
        .unwrap_or(session);
    let saved = workspace.get("layout").unwrap_or(workspace);
    let mut layout = Layout::default();
    let mut mapping = BTreeMap::new();
    if let Some(tabs) = saved["tabs"].as_array() {
        for tab in tabs {
            let id = text(tab, "id").to_owned();
            if id.is_empty() || layout.tabs.iter().any(|other| other.tab_id == id) { return Err("Invalid legacy tab identity".into()); }
            let state = saved["layoutsByTabId"].get(&id).or_else(|| tab.get("terminal")).unwrap_or(tab);
            let tree = state.get("root").or_else(|| state.get("paneTree"));
            let pane_tree = match tree {
                Some(tree) => convert_node(tree, state, workspace, tab, &mut layout, &mut mapping)?,
                None => { let content = content(state, workspace, tab, "")?; layout.mint(content) },
            };
            let active = mapping.get(text(state, "activeLeafId")).copied().filter(|id| contains(&pane_tree, *id)).unwrap_or_else(|| first(&pane_tree));
            let expanded = mapping.get(text(state, "expandedLeafId")).copied().filter(|id| contains(&pane_tree, *id));
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
    Ok((layout, mapping))
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
            let file = pane.get("file").unwrap_or(pane);
            Content::File { path: file["path"].as_str().or_else(|| tab["path"].as_str()).unwrap_or("").into(), line: file["line"].as_u64(), col: file["col"].as_u64() }
        },
        "browser" => Content::Browser { url: pane["browser"]["url"].as_str().or_else(|| pane["url"].as_str()).or_else(|| tab["browser"]["url"].as_str()).unwrap_or("").into() },
        "dag" => Content::Dag,
        "changes" => Content::Changes,
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
            assert_eq!(view.migration_map["visual"], 1);
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
