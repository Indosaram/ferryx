use ferryx_lib::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path, PersistedLayout,
    PersistedTab, PersistedWorkspace, PersistedWorkspaceSession, PersistedWorktree,
};
use std::collections::HashMap;
use std::fs;
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
        timestamp: 1787292000,
        active_workspace_id: "default".to_string(),
        workspaces,
        extra: HashMap::new(),
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

#[test]
fn test_version_2_session_envelope_restore_preserves_multiple_workspaces() {
    let dir = tempdir().unwrap();
    let session_file = dir.path().join("session_state.json");

    let v2_payload = serde_json::json!({
        "version": 2,
        "timestamp": 1787292000000u64,
        "activeWorkspaceId": "alpha",
        "workspaces": {
            "alpha": {
                "workspaceId": "alpha",
                "repoRoot": "/workspace/alpha",
                "worktrees": [
                    {
                        "path": "/workspace/alpha",
                        "branch": "refs/heads/main",
                        "head": "111111",
                        "isMain": true,
                        "isLocked": false
                    },
                    {
                        "path": "/workspace/alpha/feature",
                        "branch": "refs/heads/feature-a",
                        "head": "222222",
                        "isMain": false,
                        "isLocked": false
                    }
                ],
                "activeWorktreePath": "/workspace/alpha",
                "layout": {
                    "splitMode": "vertical",
                    "primaryTabId": "tab-term-1",
                    "secondaryTabId": "tab-browser-1",
                    "activeTabId": "tab-term-1",
                    "tabs": [
                        {
                            "id": "tab-term-1",
                            "kind": "terminal",
                            "label": "Terminal 1",
                            "pinned": false,
                            "terminal": {
                                "primarySessionId": "sess-alpha-1",
                                "paneTree": {
                                    "type": "leaf",
                                    "leafId": "leaf-term-1"
                                },
                                "sessionIdsByLeafId": {
                                    "leaf-term-1": "sess-alpha-1"
                                },
                                "activeLeafId": "leaf-term-1",
                                "expandedLeafId": null
                            }
                        },
                        {
                            "id": "tab-browser-1",
                            "kind": "browser",
                            "label": "Docs",
                            "pinned": true,
                            "browser": {
                                "browserId": "browser-alpha-1",
                                "url": "https://example.com/docs",
                                "title": "Documentation",
                                "loading": false,
                                "canGoBack": true,
                                "canGoForward": false
                            }
                        }
                    ],
                    "tabGroups": [
                        {
                            "id": "group-alpha-1",
                            "tabIds": ["tab-term-1", "tab-browser-1"],
                            "activeTabId": "tab-term-1"
                        }
                    ],
                    "tabGroupLayout": null,
                    "focusedGroupId": "group-alpha-1"
                },
                "terminalSessions": {
                    "sess-alpha-1": {
                        "localSessionId": "sess-alpha-1",
                        "backendSessionId": "backend-pty-alpha",
                        "worktreePath": "/workspace/alpha",
                        "cwd": "/workspace/alpha",
                        "createdAt": 1787292000000u64
                    }
                }
            },
            "beta": {
                "workspaceId": "beta",
                "repoRoot": "/workspace/beta",
                "worktrees": [
                    {
                        "path": "/workspace/beta",
                        "branch": "refs/heads/main",
                        "head": "333333",
                        "isMain": true,
                        "isLocked": false
                    }
                ],
                "activeWorktreePath": "/workspace/beta",
                "layout": {
                    "splitMode": "none",
                    "primaryTabId": "tab-beta-1",
                    "secondaryTabId": null,
                    "activeTabId": "tab-beta-1",
                    "tabs": [
                        {
                            "id": "tab-beta-1",
                            "kind": "terminal",
                            "label": "Beta Terminal",
                            "pinned": false,
                            "terminal": {
                                "primarySessionId": "sess-beta-1",
                                "paneTree": {
                                    "type": "leaf",
                                    "leafId": "leaf-beta-1"
                                },
                                "sessionIdsByLeafId": {
                                    "leaf-beta-1": "sess-beta-1"
                                },
                                "activeLeafId": "leaf-beta-1",
                                "expandedLeafId": null
                            }
                        }
                    ],
                    "tabGroups": [],
                    "tabGroupLayout": null,
                    "focusedGroupId": null
                },
                "terminalSessions": {
                    "sess-beta-1": {
                        "localSessionId": "sess-beta-1",
                        "backendSessionId": null,
                        "worktreePath": "/workspace/beta",
                        "cwd": "/workspace/beta",
                        "createdAt": 1787292000000u64
                    }
                }
            }
        }
    });

    let json_bytes = serde_json::to_vec_pretty(&v2_payload).unwrap();
    fs::write(&session_file, json_bytes).unwrap();
    assert!(session_file.exists());

    let loaded = load_session_from_path(&session_file)
        .expect("loader should not return io error")
        .expect(
            "native session loader must accept version-2 session envelope and restore workspaces",
        );

    assert_eq!(loaded.version, 2);
    assert_eq!(loaded.active_workspace_id, "alpha");
    assert_eq!(loaded.workspaces.len(), 2);
    assert!(
        loaded.workspaces.contains_key("alpha"),
        "workspace alpha must survive"
    );
    assert!(
        loaded.workspaces.contains_key("beta"),
        "workspace beta must survive"
    );
    assert_eq!(
        loaded.workspaces["alpha"].repo_root,
        PathBuf::from("/workspace/alpha")
    );
    assert_eq!(
        loaded.workspaces["beta"].repo_root,
        PathBuf::from("/workspace/beta")
    );
    assert_eq!(loaded.workspaces["alpha"].worktrees.len(), 2);
    assert_eq!(loaded.workspaces["beta"].worktrees.len(), 1);
}

#[test]
fn test_v2_v3_session_roundtrip_preserves_all_typed_fields() {
    let dir = tempdir().unwrap();
    let initial_file = dir.path().join("session_initial.json");
    let resaved_file = dir.path().join("session_resaved.json");

    let v2_payload = serde_json::json!({
        "version": 2,
        "timestamp": 1787292000000u64,
        "activeWorkspaceId": "alpha",
        "workspaces": {
            "alpha": {
                "workspaceId": "alpha",
                "repoRoot": "/workspace/alpha",
                "worktrees": [
                    {
                        "path": "/workspace/alpha",
                        "branch": "refs/heads/main",
                        "head": "111111",
                        "isMain": true,
                        "isLocked": false
                    }
                ],
                "activeWorktreePath": "/workspace/alpha",
                "layout": {
                    "splitMode": "vertical",
                    "primaryTabId": "tab-term-1",
                    "secondaryTabId": "tab-browser-1",
                    "activeTabId": "tab-term-1",
                    "tabs": [
                        {
                            "id": "tab-term-1",
                            "kind": "terminal",
                            "label": "Terminal 1",
                            "pinned": false,
                            "terminal": {
                                "primarySessionId": "sess-alpha-1",
                                "paneTree": {
                                    "type": "leaf",
                                    "leafId": "leaf-term-1"
                                },
                                "sessionIdsByLeafId": {
                                    "leaf-term-1": "sess-alpha-1"
                                },
                                "activeLeafId": "leaf-term-1",
                                "expandedLeafId": "leaf-term-1"
                            }
                        },
                        {
                            "id": "tab-browser-1",
                            "kind": "browser",
                            "label": "Docs",
                            "pinned": true,
                            "browser": {
                                "browserId": "browser-alpha-1",
                                "url": "https://example.com/docs",
                                "title": "Documentation",
                                "loading": false,
                                "canGoBack": true,
                                "canGoForward": false,
                                "profileId": "custom-profile",
                                "worktreePath": "/workspace/alpha",
                                "worktreeLabel": "alpha-main"
                            }
                        }
                    ],
                    "tabGroups": [
                        {
                            "id": "group-alpha-1",
                            "tabIds": ["tab-term-1", "tab-browser-1"],
                            "activeTabId": "tab-term-1"
                        }
                    ],
                    "tabGroupLayout": {
                        "type": "group",
                        "groupId": "group-alpha-1"
                    },
                    "focusedGroupId": "group-alpha-1",
                    "layoutsByTabId": {
                        "tab-term-1": {
                            "root": {
                                "type": "leaf",
                                "leafId": "leaf-term-1"
                            },
                            "activeLeafId": "leaf-term-1",
                            "sessionIdsByLeafId": {
                                "leaf-term-1": "sess-alpha-1"
                            }
                        }
                    }
                },
                "terminalSessions": {
                    "sess-alpha-1": {
                        "localSessionId": "sess-alpha-1",
                        "backendSessionId": "backend-pty-alpha",
                        "worktreePath": "/workspace/alpha",
                        "cwd": "/workspace/alpha",
                        "createdAt": 1787292000000u64
                    }
                },
                "worktreeLayouts": {
                    "/workspace/alpha": {
                        "splitMode": "none",
                        "primaryTabId": "tab-term-1",
                        "secondaryTabId": null,
                        "activeTabId": "tab-term-1",
                        "tabs": []
                    }
                },
                "layoutByWorktree": {
                    "/workspace/alpha": {
                        "type": "group",
                        "groupId": "group-alpha-1"
                    }
                }
            }
        }
    });

    let json_bytes = serde_json::to_vec_pretty(&v2_payload).unwrap();
    fs::write(&initial_file, json_bytes).unwrap();

    // 1. First load
    let loaded = load_session_from_path(&initial_file)
        .expect("initial load should succeed")
        .expect("session must be present");

    // 2. Save loaded session to resaved_file
    save_session_to_path(&resaved_file, &loaded).expect("save_session_to_path should succeed");
    assert!(resaved_file.exists());

    // 3. Second load from resaved_file
    let reloaded = load_session_from_path(&resaved_file)
        .expect("reloaded load should succeed")
        .expect("reloaded session must be present");

    assert_eq!(reloaded.version, 2);
    assert_eq!(reloaded.active_workspace_id, "alpha");
    assert!(reloaded.workspaces.contains_key("alpha"));

    // 4. Assert typed details in the serialized / reloaded structure
    let resaved_content = fs::read_to_string(&resaved_file).unwrap();
    let resaved_json: serde_json::Value = serde_json::from_str(&resaved_content).unwrap();

    let alpha = &resaved_json["workspaces"]["alpha"];
    assert_eq!(alpha["workspaceId"], "alpha");
    assert_eq!(alpha["repoRoot"], "/workspace/alpha");

    // Browser tab metadata retention
    let tabs = alpha["layout"]["tabs"].as_array().expect("tabs array");
    let browser_tab = tabs
        .iter()
        .find(|t| t["id"] == "tab-browser-1")
        .expect("browser tab must survive roundtrip");
    assert_eq!(browser_tab["kind"], "browser");
    assert_eq!(browser_tab["pinned"], true);
    assert_eq!(browser_tab["browser"]["browserId"], "browser-alpha-1");
    assert_eq!(browser_tab["browser"]["url"], "https://example.com/docs");
    assert_eq!(browser_tab["browser"]["title"], "Documentation");
    assert_eq!(browser_tab["browser"]["canGoBack"], true);
    assert_eq!(browser_tab["browser"]["profileId"], "custom-profile");
    assert_eq!(browser_tab["browser"]["worktreePath"], "/workspace/alpha");
    assert_eq!(browser_tab["browser"]["worktreeLabel"], "alpha-main");

    // Terminal tab metadata retention
    let term_tab = tabs
        .iter()
        .find(|t| t["id"] == "tab-term-1")
        .expect("terminal tab must survive roundtrip");
    assert_eq!(term_tab["kind"], "terminal");
    assert_eq!(term_tab["terminal"]["primarySessionId"], "sess-alpha-1");
    assert_eq!(term_tab["terminal"]["paneTree"]["leafId"], "leaf-term-1");
    assert_eq!(
        term_tab["terminal"]["sessionIdsByLeafId"]["leaf-term-1"],
        "sess-alpha-1"
    );
    assert_eq!(term_tab["terminal"]["activeLeafId"], "leaf-term-1");

    // Layout metadata retention (tabGroups, tabGroupLayout, focusedGroupId, layoutsByTabId)
    assert!(alpha["layout"]["tabGroups"].is_array());
    assert_eq!(alpha["layout"]["tabGroups"][0]["id"], "group-alpha-1");
    assert_eq!(
        alpha["layout"]["tabGroupLayout"]["groupId"],
        "group-alpha-1"
    );
    assert_eq!(alpha["layout"]["focusedGroupId"], "group-alpha-1");
    assert_eq!(
        alpha["layout"]["layoutsByTabId"]["tab-term-1"]["root"]["leafId"],
        "leaf-term-1"
    );

    // Terminal sessions local/backend IDs retention
    assert!(
        !loaded.workspaces["alpha"]
            .terminal_sessions
            .keys()
            .any(|k| k.starts_with("__ferryx_meta:")),
        "terminal_sessions must not contain internal metadata keys in loaded session"
    );
    assert!(
        !reloaded.workspaces["alpha"]
            .terminal_sessions
            .keys()
            .any(|k| k.starts_with("__ferryx_meta:")),
        "terminal_sessions must not contain internal metadata keys in reloaded session"
    );
    let term_sess = &alpha["terminalSessions"]["sess-alpha-1"];
    assert_eq!(term_sess["localSessionId"], "sess-alpha-1");
    assert_eq!(term_sess["backendSessionId"], "backend-pty-alpha");
    assert_eq!(term_sess["worktreePath"], "/workspace/alpha");
    assert_eq!(term_sess["cwd"], "/workspace/alpha");

    // Worktree layouts and layoutByWorktree retention
    assert!(alpha["worktreeLayouts"].is_object());
    assert_eq!(
        alpha["worktreeLayouts"]["/workspace/alpha"]["splitMode"],
        "none"
    );
    assert_eq!(
        alpha["layoutByWorktree"]["/workspace/alpha"]["groupId"],
        "group-alpha-1"
    );
}
