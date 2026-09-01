use crate::ipc::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorktree {
    pub path: String,
    pub branch: String,
    pub head: String,
    #[serde(default)]
    pub is_main: bool,
    #[serde(default)]
    pub is_locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSshPane {
    pub host_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTerminalTabState {
    pub primary_session_id: String,
    pub pane_tree: serde_json::Value,
    pub session_ids_by_leaf_id: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub ssh_by_leaf_id: HashMap<String, PersistedSshPane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_leaf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_leaf_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedBrowserTabState {
    pub browser_id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_go_back: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_go_forward: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "profile")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTabGroup {
    pub id: String,
    pub tab_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTab {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<PersistedTerminalTabState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<PersistedBrowserTabState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,

    // v1 compatibility fields
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_tree: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ids_by_leaf_id: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_leaf_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_leaf_id: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLayout {
    pub split_mode: String,
    #[serde(default)]
    pub primary_tab_id: Option<String>,
    #[serde(default)]
    pub secondary_tab_id: Option<String>,
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub tabs: Vec<PersistedTab>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_groups: Option<Vec<PersistedTabGroup>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_group_layout: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layouts_by_tab_id: Option<serde_json::Value>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTerminalSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_session_id: Option<String>,
    #[serde(default)]
    pub worktree_path: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_scrollback: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorkspace {
    pub workspace_id: String,
    pub repo_root: PathBuf,
    #[serde(default)]
    pub worktrees: Vec<PersistedWorktree>,
    #[serde(default)]
    pub active_worktree_path: Option<String>,
    pub layout: PersistedLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_layouts: Option<HashMap<String, PersistedLayout>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_by_worktree: Option<serde_json::Value>,
    #[serde(default)]
    pub terminal_sessions: HashMap<String, PersistedTerminalSession>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorkspaceSession {
    pub version: u32,
    pub timestamp: u64,
    pub active_workspace_id: String,
    pub workspaces: HashMap<String, PersistedWorkspace>,
    #[serde(default, flatten, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, serde_json::Value>,
}

pub fn save_session_to_path(
    path: &Path,
    session: &PersistedWorkspaceSession,
) -> Result<(), IpcError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to create directory for session state: {}", e),
            )
        })?;
    }

    let serialized = serde_json::to_string_pretty(session).map_err(|e| {
        IpcError::new(
            IpcErrorCode::ParseError,
            format!("Failed to serialize session state: {}", e),
        )
    })?;

    let tmp_path = path.with_extension("json.tmp");

    // Durable atomic write: write -> sync_all -> rename -> parent fsync
    {
        use std::io::Write;
        let mut file = File::create(&tmp_path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to create temporary session file: {}", e),
            )
        })?;
        file.write_all(serialized.as_bytes()).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to write session state: {}", e),
            )
        })?;
        file.sync_all().map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to fsync temporary session file: {}", e),
            )
        })?;
    }

    fs::rename(&tmp_path, path).map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to atomically rename session state file: {}", e),
        )
    })?;

    // Parent directory fsync
    if let Some(parent) = path.parent() {
        if let Ok(dir_file) = File::open(parent) {
            let _ = dir_file.sync_all();
        }
    }

    Ok(())
}

pub fn load_session_from_path(path: &Path) -> Result<Option<PersistedWorkspaceSession>, IpcError> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path).map_err(|e| {
        IpcError::new(
            IpcErrorCode::IoError,
            format!("Failed to read session state file: {}", e),
        )
    })?;

    match serde_json::from_str::<PersistedWorkspaceSession>(&content) {
        Ok(session) => {
            if !(1..=3).contains(&session.version) {
                eprintln!(
                    "Warning: Unsupported session version {}, ignoring.",
                    session.version
                );
                return Ok(None);
            }
            Ok(Some(session))
        }
        Err(err) => {
            let backup_path = path.with_extension("json.corrupted");
            let _ = fs::rename(path, backup_path);
            eprintln!(
                "Warning: Session state file was corrupted ({}), moved to backup.",
                err
            );
            Ok(None)
        }
    }
}

pub fn clear_session_from_path(path: &Path) -> Result<(), IpcError> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to remove session state file: {}", e),
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_session_save_load_clear_cycle() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        let mut workspaces = HashMap::new();
        workspaces.insert(
            "default".to_string(),
            PersistedWorkspace {
                workspace_id: "default".to_string(),
                repo_root: PathBuf::from("/tmp/repo"),
                worktrees: vec![PersistedWorktree {
                    path: "/tmp/repo".to_string(),
                    branch: "main".to_string(),
                    head: "abcdef".to_string(),
                    is_main: true,
                    is_locked: false,
                }],
                active_worktree_path: Some("/tmp/repo".to_string()),
                layout: PersistedLayout {
                    split_mode: "none".to_string(),
                    primary_tab_id: Some("tab-1".to_string()),
                    secondary_tab_id: None,
                    active_tab_id: Some("tab-1".to_string()),
                    tabs: vec![PersistedTab {
                        id: "tab-1".to_string(),
                        kind: None,
                        label: "main".to_string(),
                        pinned: None,
                        terminal: None,
                        browser: None,
                        custom_title: None,
                        session_id: Some("term-1".to_string()),
                        worktree_path: Some("/tmp/repo".to_string()),
                        pane_tree: None,
                        session_ids_by_leaf_id: None,
                        active_leaf_id: None,
                        expanded_leaf_id: None,
                        extra: HashMap::new(),
                    }],
                    tab_groups: None,
                    tab_group_layout: None,
                    focused_group_id: None,
                    layouts_by_tab_id: None,
                    extra: HashMap::new(),
                },
                worktree_layouts: None,
                layout_by_worktree: None,
                terminal_sessions: HashMap::new(),
                extra: HashMap::new(),
            },
        );

        let session = PersistedWorkspaceSession {
            version: 1,
            timestamp: 1234567890,
            active_workspace_id: "default".to_string(),
            workspaces,
            extra: HashMap::new(),
        };

        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_none());

        save_session_to_path(&session_file, &session).unwrap();
        assert!(session_file.exists());

        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_some());
        let loaded_session = loaded.unwrap();
        assert_eq!(loaded_session.version, 1);
        assert_eq!(loaded_session.active_workspace_id, "default");
        assert_eq!(
            loaded_session.workspaces["default"].repo_root,
            PathBuf::from("/tmp/repo")
        );

        clear_session_from_path(&session_file).unwrap();
        assert!(!session_file.exists());
        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn red_v3_session_round_trips_ssh_pane_fields() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        let mut ssh_by_leaf_id = HashMap::new();
        ssh_by_leaf_id.insert(
            "leaf-1".to_string(),
            PersistedSshPane {
                host_id: "h-box".to_string(),
                remote_path: Some("/home/sook/proj".to_string()),
            },
        );

        let session = PersistedWorkspaceSession {
            version: 3,
            timestamp: 1,
            active_workspace_id: "default".to_string(),
            workspaces: HashMap::from([(
                "default".to_string(),
                PersistedWorkspace {
                    workspace_id: "default".to_string(),
                    repo_root: PathBuf::from("/tmp/repo"),
                    layout: PersistedLayout {
                        split_mode: "none".to_string(),
                        tabs: vec![PersistedTab {
                            id: "tab-1".to_string(),
                            label: "ssh".to_string(),
                            terminal: Some(PersistedTerminalTabState {
                                primary_session_id: "s1".to_string(),
                                pane_tree: serde_json::json!({}),
                                session_ids_by_leaf_id: HashMap::from([(
                                    "leaf-1".to_string(),
                                    "s1".to_string(),
                                )]),
                                ssh_by_leaf_id,
                                ..Default::default()
                            }),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]),
            extra: HashMap::new(),
        };

        save_session_to_path(&session_file, &session).unwrap();
        let loaded = load_session_from_path(&session_file)
            .unwrap()
            .expect("v3 session loads");
        assert_eq!(loaded.version, 3);
        let tab = &loaded.workspaces["default"].layout.tabs[0];
        let terminal = tab.terminal.as_ref().expect("terminal tab state");
        let pane = terminal
            .ssh_by_leaf_id
            .get("leaf-1")
            .expect("ssh pane fields survive a save/load cycle");
        assert_eq!(pane.host_id, "h-box");
        assert_eq!(pane.remote_path.as_deref(), Some("/home/sook/proj"));
    }

    #[test]
    fn red_v2_session_without_ssh_fields_loads_with_empty_map() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");
        fs::write(
            &session_file,
            r#"{
                "version": 2,
                "timestamp": 1,
                "activeWorkspaceId": "default",
                "workspaces": {
                    "default": {
                        "workspaceId": "default",
                        "repoRoot": "/tmp/repo",
                        "layout": {
                            "splitMode": "none",
                            "tabs": [
                                {
                                    "id": "tab-1",
                                    "label": "main",
                                    "terminal": {
                                        "primarySessionId": "s1",
                                        "paneTree": {},
                                        "sessionIdsByLeafId": {}
                                    }
                                }
                            ]
                        },
                        "terminalSessions": {}
                    }
                }
            }"#,
        )
        .unwrap();

        let loaded = load_session_from_path(&session_file)
            .unwrap()
            .expect("v2 session still loads (backward compatible)");
        assert_eq!(loaded.version, 2);
        let tab = &loaded.workspaces["default"].layout.tabs[0];
        let terminal = tab.terminal.as_ref().expect("terminal tab state");
        assert!(terminal.ssh_by_leaf_id.is_empty());
    }

    #[test]
    fn test_corrupted_session_recovery() {
        let dir = tempdir().unwrap();
        let session_file = dir.path().join("session_state.json");

        fs::write(&session_file, "{ broken json ...").unwrap();

        let loaded = load_session_from_path(&session_file).unwrap();
        assert!(loaded.is_none());
        assert!(!session_file.exists());
        assert!(dir.path().join("session_state.json.corrupted").exists());
    }
}
