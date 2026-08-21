use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
use ferryx_lib::daemon::server::{get_socket_path, DaemonServer};
use ferryx_lib::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path, PersistedLayout,
    PersistedTab, PersistedWorkspace, PersistedWorkspaceSession, PersistedWorktree,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test]
async fn test_daemon_uds_handshake_and_ping() {
    let server = Arc::new(DaemonServer::new());
    let server_clone = Arc::clone(&server);
    tokio::spawn(async move {
        let _ = server_clone.run_server().await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    let socket_path = get_socket_path();

    if let Ok(stream) = UnixStream::connect(&socket_path).await {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        // 1. Handshake
        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs).unwrap();
        hs_json.push('\n');
        write_half.write_all(hs_json.as_bytes()).await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        match resp {
            DaemonResponse::HandshakeOk { version, pid } => {
                assert_eq!(version, DAEMON_PROTOCOL_VERSION);
                assert!(pid > 0);
            }
            _ => panic!("Expected HandshakeOk"),
        }

        // 2. Ping
        line.clear();
        let ping = DaemonRequest::Ping;
        let mut ping_json = serde_json::to_string(&ping).unwrap();
        ping_json.push('\n');
        write_half.write_all(ping_json.as_bytes()).await.unwrap();

        reader.read_line(&mut line).await.unwrap();
        let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, DaemonResponse::Pong));
    }
}

#[tokio::test]
async fn test_durable_fsync_session_persistence_lifecycle() {
    let dir = tempdir().unwrap();
    let session_file = dir.path().join("durable_session.json");

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
                    session_id: "term-1".to_string(),
                    label: "main".to_string(),
                    custom_title: None,
                    worktree_path: "/tmp/repo".to_string(),
                    pane_tree: None,
                    session_ids_by_leaf_id: None,
                    active_leaf_id: None,
                }],
            },
            terminal_sessions: HashMap::new(),
        },
    );

    let session = PersistedWorkspaceSession {
        version: 1,
        timestamp: 1234567890,
        active_workspace_id: "default".to_string(),
        workspaces,
    };

    save_session_to_path(&session_file, &session).expect("durable fsync save");
    assert!(session_file.exists());

    let loaded = load_session_from_path(&session_file)
        .expect("load")
        .expect("present");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.active_workspace_id, "default");

    clear_session_from_path(&session_file).expect("clear");
    assert!(!session_file.exists());
}
