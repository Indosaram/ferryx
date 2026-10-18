//! Wire-level tests for the control-v1 channel and its effect on the data protocol (todo 15).
//! A child of server.rs (through control_server.rs), so it drives a real DaemonServer.

use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Sends one data-protocol request over handle_client and returns the one-line response.
async fn data_exchange(server: &Arc<DaemonServer>, request: DaemonRequest) -> DaemonResponse {
    let (client, server_side) = tokio::io::duplex(64 * 1024);
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut json = serde_json::to_string(&request).unwrap();
    json.push('\n');
    write_half.write_all(json.as_bytes()).await.unwrap();
    let mut line = String::new();
    BufReader::new(read_half).read_line(&mut line).await.unwrap();
    drop(write_half);
    serving.abort();
    serde_json::from_str(line.trim()).unwrap()
}

#[tokio::test]
async fn control_retiring_spawn_answers_typed_daemon_retiring_code() {
    let server = Arc::new(DaemonServer::new());
    server
        .handover_manager
        .control_retire_when_idle(&server.terminal_service);
    let response = data_exchange(
        &server,
        DaemonRequest::Spawn {
            client_request_id: "retiring-spawn".into(),
            workspace_id: "ws-retiring".into(),
            worktree: None,
            cwd: None,
            cols: 80,
            rows: 24,
            shell: None,
            startup: None,
            local_split: None,
            session_id: None,
        },
    )
    .await;
    match response {
        DaemonResponse::Error { code, .. } => {
            assert_eq!(code.as_deref(), Some(crate::daemon::control::CODE_DAEMON_RETIRING));
        }
        other => panic!("expected a typed DAEMON_RETIRING error, got {other:?}"),
    }
}
