use orca_lite_lib::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path, PersistedLayout,
    PersistedTab, PersistedWorkspace, PersistedWorkspaceSession, PersistedWorktree,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_session_lifecycle_integration() {
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
                tabs: vec![PersistedTab {
                    id: "tab-1".to_string(),
                    session_id: "term-1".to_string(),
                    label: "main".to_string(),
                    custom_title: None,
                    worktree_path: "/tmp/repo".to_string(),
                }],
            },
            terminal_sessions: HashMap::new(),
        },
    );

    let session = PersistedWorkspaceSession {
        version: 1,
        timestamp: 1787292000,
        active_workspace_id: "default".to_string(),
        workspaces,
    };

    let loaded = load_session_from_path(&session_file).unwrap();
    assert!(loaded.is_none());

    save_session_to_path(&session_file, &session).unwrap();
    assert!(session_file.exists());

    let loaded = load_session_from_path(&session_file).unwrap().unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.active_workspace_id, "default");
    assert_eq!(
        loaded.workspaces["default"].repo_root,
        PathBuf::from("/tmp/repo")
    );

    clear_session_from_path(&session_file).unwrap();
    assert!(!session_file.exists());
}
