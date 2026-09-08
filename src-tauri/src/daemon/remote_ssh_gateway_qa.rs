use super::*;
use crate::remote::{DevicePermission, RemoteActiveDesktopSelection, RemoteWorkspaceState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) async fn exercise(daemon: &DaemonServer, workspace: &str, session: &str, store: &Path) {
    let state = daemon.remote_state();
    *state.ssh_store_path.write() = Some(store.to_path_buf());
    state.clear_active_selection();
    let pin = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (token, _) = state.auth_manager.exchange_pairing_code(&pin, "SSH gateway QA").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let gateway = Arc::clone(state);
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, crate::remote::create_remote_router(gateway)).await.unwrap();
    }));
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let response = client.get(format!("http://{addr}/api/v1/workspace/state"))
        .bearer_auth(&token).send().await.unwrap();
    assert!(response.status().is_success());
    let body = response.text().await.unwrap();
    let inventory: RemoteWorkspaceState = serde_json::from_str(&body).unwrap();
    assert!(inventory.projects.iter().any(|project| project.workspace_id == workspace));
    assert!(inventory.sessions.iter().any(|item| item.session_id == session
        && item.workspace_id.as_deref() == Some(workspace)));

    let url = format!("http://{addr}/api/v1/terminal/{session}");
    let handshake = || client.get(&url).bearer_auth(&token)
        .header("connection", "upgrade").header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==");
    assert_eq!(handshake().send().await.unwrap().status().as_u16(), 403);
    let mut events = state.event_tx.subscribe();
    let response = client.post(format!("http://{addr}/api/v1/workspace/select"))
        .bearer_auth(&token).header("content-type", "application/json")
        .body(serde_json::json!({ "workspaceId": workspace, "sessionId": session }).to_string())
        .send().await.unwrap();
    assert!(response.status().is_success());
    let event: crate::remote::protocol::RemoteEventMessage =
        serde_json::from_str(&events.try_recv().unwrap()).unwrap();
    assert_eq!(event.payload["sessionId"], session);
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some(workspace.to_owned()), session_id: Some(session.to_owned()),
        ..Default::default()
    });
    let response = handshake().send().await.unwrap();
    assert_eq!(response.status().as_u16(), 101);
    let mut socket = response.upgrade().await.unwrap();
    let input = b"printf '\\106\\105\\122\\122\\131\\130\\055\\122\\105\\115\\117\\124\\105\\055\\117\\113\\n'\n";
    let mut frame = vec![0x82, 0x80 | u8::try_from(input.len()).unwrap(), 0, 0, 0, 0];
    frame.extend_from_slice(input);
    socket.write_all(&frame).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut output = Vec::new();
        loop {
            let mut header = [0; 2];
            socket.read_exact(&mut header).await.unwrap();
            let length = match header[1] & 127 {
                126 => u64::from(socket.read_u16().await.unwrap()),
                127 => socket.read_u64().await.unwrap(),
                value => u64::from(value),
            };
            assert!(length < 1024 * 1024);
            let mut payload = vec![0; usize::try_from(length).unwrap()];
            socket.read_exact(&mut payload).await.unwrap();
            assert_ne!(header[0] & 15, 8);
            if header[0] & 15 == 2 {
                output.extend(payload);
                if String::from_utf8_lossy(&output).contains("FERRYX-REMOTE-OK") { break; }
            }
        }
    }).await.expect("actual SSH output through remote WebSocket");
    println!("FERRYX_REMOTE_SSH_QA_OK inventory selection active-lock websocket-input-output");
}
