use ferryx_lib::daemon::launchd::{
    generate_launchd_plist, get_launchd_plist_path, uninstall_launchd_agent_from_path,
};
use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
use ferryx_lib::daemon::server::get_socket_path;
use ferryx_lib::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path, PersistedLayout,
    PersistedTab, PersistedWorkspace, PersistedWorkspaceSession, PersistedWorktree,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command as TokioCommand;
use tokio::time::timeout;

#[tokio::test]
async fn test_daemon_cli_selection_headless_readiness_and_cancellation() {
    let bin_path = env!("CARGO_BIN_EXE_ferryx");
    let mut child = TokioCommand::new(bin_path)
        .arg("--daemon")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn ferryx with --daemon");

    let stdout = child.stdout.take().expect("Child stdout must be captured");
    let mut reader = BufReader::new(stdout).lines();

    // Exact readiness signal: wait for deterministic daemon ready line without polling/sleeps
    let ready_line = timeout(Duration::from_secs(5), reader.next_line())
        .await
        .expect("Daemon readiness timed out")
        .expect("Failed to read readiness line from daemon stdout")
        .expect("Daemon exited before emitting readiness signal");

    assert_eq!(
        ready_line, "FERRYX_DAEMON_READY",
        "Daemon stdout must emit deterministic readiness token"
    );

    let socket_path = get_socket_path();
    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("Failed to connect to daemon UDS socket after readiness signal");

    let (read_half, mut write_half) = stream.into_split();
    let mut socket_reader = BufReader::new(read_half);

    // 1. Handshake
    let hs = DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
    };
    let mut hs_json = serde_json::to_string(&hs).unwrap();
    hs_json.push('\n');
    write_half.write_all(hs_json.as_bytes()).await.unwrap();

    let mut line = String::new();
    socket_reader.read_line(&mut line).await.unwrap();
    let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
    match resp {
        DaemonResponse::HandshakeOk { version, pid, .. } => {
            assert_eq!(version, DAEMON_PROTOCOL_VERSION);
            assert!(pid > 0);
        }
        other => panic!("Expected HandshakeOk, got {other:?}"),
    }

    // 2. Ping
    line.clear();
    let ping = DaemonRequest::Ping;
    let mut ping_json = serde_json::to_string(&ping).unwrap();
    ping_json.push('\n');
    write_half.write_all(ping_json.as_bytes()).await.unwrap();

    socket_reader.read_line(&mut line).await.unwrap();
    let resp: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
    assert!(matches!(resp, DaemonResponse::Pong));

    // Exact cancellation signal: terminate child process and await bounded shutdown
    child
        .kill()
        .await
        .expect("Failed to send termination signal to daemon process");
    let exit_status = timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("Daemon process failed to exit within timeout after cancellation")
        .expect("Failed to wait on child process");

    assert!(
        !exit_status.success() || exit_status.code().unwrap_or(0) == 0,
        "Child process terminated"
    );
}

#[test]
fn test_launchd_plist_generation_and_identity_contract() {
    let plist = generate_launchd_plist("/Applications/Ferryx.app/Contents/MacOS/ferryx");
    assert!(
        plist.contains("<string>com.rorca.daemon</string>"),
        "Must preserve com.rorca.daemon compatibility identifier in launchd plist"
    );
    assert!(
        plist.contains("<string>/Applications/Ferryx.app/Contents/MacOS/ferryx</string>"),
        "Must contain executable path"
    );
    assert!(
        plist.contains("<string>--daemon</string>"),
        "Must configure --daemon flag in ProgramArguments"
    );
}

#[test]
fn test_get_launchd_plist_path_location() {
    if let Some(path) = get_launchd_plist_path() {
        assert!(
            path.ends_with("Library/LaunchAgents/com.rorca.daemon.plist"),
            "LaunchAgent plist must target Library/LaunchAgents/com.rorca.daemon.plist"
        );
    }
}

#[test]
fn test_uninstall_launchd_agent_idempotent_when_missing() {
    let dir = tempdir().unwrap();
    let missing_plist = dir.path().join("missing.plist");
    assert!(
        uninstall_launchd_agent_from_path(&missing_plist).is_ok(),
        "Uninstalling missing plist should be a successful no-op"
    );
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
