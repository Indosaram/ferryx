//! Test-only external-machine entry point. No production trust override exists.
use super::*;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type TestWsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn read_next_ws_json(ws: &mut TestWsStream) -> serde_json::Value {
    loop {
        let raw = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("timeout waiting for websocket frame")
            .expect("stream ended unexpectedly")
            .expect("websocket frame error");
        match raw {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                return serde_json::from_str(&text).expect("valid json text frame");
            }
            tokio_tungstenite::tungstenite::Message::Ping(_)
            | tokio_tungstenite::tungstenite::Message::Pong(_) => continue,
            other => panic!("expected text websocket frame, got: {other:?}"),
        }
    }
}

fn extract_grid_text(frame: &serde_json::Value) -> String {
    let mut text = String::new();
    if let Some(lines) = frame.get("lines").and_then(|l| l.as_array()) {
        for line in lines {
            if let Some(runs) = line.get("runs").and_then(|r| r.as_array()) {
                for run in runs {
                    if let Some(t) = run.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                    }
                }
            }
            text.push('\n');
        }
    }
    text
}

pub(super) const CONFIG_ENV: &str = "FERRYX_SSH_QA_CONFIG";
const CHILD_ENV: &str = "FERRYX_SSH_QA_CHILD";
const TEST_NAME: &str =
    "daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct QaConfig {
    pub host: SshHost,
    pub known_hosts_file: PathBuf,
    pub repo_path: String,
    pub expected_repo_root: String,
    pub expected_git_root: Option<String>,
}

pub(super) async fn run(config_path: &Path) {
    assert!(
        config_path.is_absolute(),
        "QA configuration path must be absolute"
    );
    let config: QaConfig =
        serde_json::from_slice(&std::fs::read(config_path).expect("read QA configuration"))
            .expect("parse QA configuration");
    assert!(
        config.known_hosts_file.is_absolute(),
        "QA trust file must be absolute"
    );
    let trust_before = std::fs::read(&config.known_hosts_file).expect("read pinned QA known_hosts");
    assert!(
        !trust_before.is_empty(),
        "QA trust file must contain the independently verified key"
    );
    crate::ssh::direct::validate_host(&config.host).expect("valid QA host");
    crate::ssh::direct::validate_remote_path(&config.repo_path).expect("valid QA remote path");
    if std::env::var_os(CHILD_ENV).is_some() {
        exercise(&config).await;
    } else {
        let launcher = tempfile::tempdir().expect("private QA launcher directory");
        let wrapper = launcher.path().join("ssh");
        std::fs::write(&wrapper, format!(
            "#!/bin/sh\nexec /usr/bin/ssh -F /dev/null -o UserKnownHostsFile={} -o GlobalKnownHostsFile=/dev/null -o StrictHostKeyChecking=yes -o BatchMode=yes -o UpdateHostKeys=no \"$@\"\n",
            crate::ssh::direct::quote_posix(config.known_hosts_file.to_str().expect("UTF-8 trust path")),
        )).expect("write isolated SSH launcher");
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = std::env::var_os("PATH").expect("PATH");
        let mut paths = vec![launcher.path().to_path_buf()];
        paths.extend(std::env::split_paths(&path));
        let output = tokio::time::timeout(
            Duration::from_secs(45),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(CONFIG_ENV, config_path)
                .env(CHILD_ENV, "1")
                .env("PATH", std::env::join_paths(paths).unwrap())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("bounded external SSH QA child")
        .expect("run SSH QA child");
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "SSH QA child failed: {}",
            output.status
        );
    }
    assert_eq!(
        std::fs::read(&config.known_hosts_file).unwrap(),
        trust_before,
        "QA must not mutate the pinned trust file"
    );
}

async fn exercise(config: &QaConfig) {
    // All bookkeeping is private to this test. No remote fixtures are created or
    // repositories mutated; VM setup/cleanup belongs to the lead.
    let state = tempfile::Builder::new()
        .prefix("fx")
        .tempdir_in("/tmp")
        .unwrap();
    let host_store = state.path().join("ssh_hosts.json");
    write_hosts(&host_store, vec![config.host.clone()]);
    let response = register_remote_project(
        host_store.clone(),
        RegisterRemoteProjectRequest {
            workspace_id: "qa-project".into(),
            host_id: config.host.id.clone(),
            repo_path: config.repo_path.clone(),
        },
    )
    .await
    .expect("actual backend registration/probe against QA host");
    assert_eq!(response.repo_root, config.expected_repo_root);
    assert_eq!(response.git_root, config.expected_git_root);
    assert_eq!(response.host_id, config.host.id);
    assert_eq!(
        projects::resolve(&host_store, &response.workspace_id)
            .unwrap()
            .0
            .repo_root,
        config.expected_repo_root
    );
    println!(
        "FERRYX_SSH_QA_REGISTERED {}",
        serde_json::to_string(&response).unwrap()
    );
    let mut daemon = DaemonServer::new_with_paths(
        Some(state.path().join("gateway.json")),
        Some(state.path().join("auth.json")),
    );
    let _helper = if config.host.hostname == "127.0.0.1" {
        Some(install_test_helper(&config.host, state.path()).await)
    } else {
        // External QA uses its explicitly preinstalled helper, never a local-path fixture.
        daemon.helper_home = None;
        None
    };
    for request in ["qa-new-tab", "qa-split-restore"] {
        let session = daemon
            .handle_spawn(
                request,
                &response.workspace_id,
                None,
                None,
                80,
                24,
                None,
                Some(TerminalStartup::RemoteSsh {
                    host_store_path: host_store.clone(),
                }),
            )
            .await
            .expect("daemon-owned SSH PTY");
        let (mut history, mut events) = daemon.terminal_service.attach(&session).unwrap();
        wait_remote_connected(&daemon, &session).await;
        daemon
            .write_session_input(
                &session,
                b"printf '\\136\\123\\123\\110\\055\\117\\113\\072'; pwd -P\n".to_vec(),
            )
            .await
            .unwrap();
        let expected = format!("^SSH-OK:{}", config.expected_repo_root);
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            while !String::from_utf8_lossy(&history).contains(&expected) {
                history.extend(events.recv().await.expect("SSH output event"));
            }
        })
        .await;

        // Grid resize verification (Step 1-9): verifies the browser SSH grid resize fix
        // reaches the remote PTY and mirror.
        assert!(result.is_ok(), "exact remote CWD through SSH PTY must succeed before grid test");

        // 1. Start gateway router on an ephemeral port and admit SSH session.
        let remote_state = Arc::clone(daemon.remote_state());
        remote_state.set_active_selection(crate::remote::RemoteActiveDesktopSelection {
            session_id: Some(session.clone()),
            ..Default::default()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port for remote router");
        let addr = listener.local_addr().expect("resolve ephemeral port");
        let router_state = Arc::clone(&remote_state);
        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                crate::remote::create_remote_router(router_state),
            )
            .await;
        });

        // 2. Mint Control-permission pairing code and exchange for device token.
        let code = remote_state
            .auth_manager
            .create_pairing_code(crate::remote::DevicePermission::Control);
        let (token, _) = remote_state
            .auth_manager
            .exchange_pairing_code(&code, "QA grid resize")
            .expect("exchange pairing code for device token");

        // 3. Open WebSocket to /api/v1/terminal/{session}?render=grid&cols=100&rows=30.
        let ws_url = format!(
            "ws://127.0.0.1:{}/api/v1/terminal/{}?render=grid&cols=100&rows=30",
            addr.port(),
            session
        );
        let mut ws_request = ws_url
            .into_client_request()
            .expect("valid websocket request");
        ws_request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .expect("valid authorization header"),
        );
        let (mut ws_stream, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async(ws_request),
        )
        .await
        .expect("connect_async within timeout")
        .expect("open grid websocket");

        // 4. Read first Text frame (remoteStatus) and parse generation, then read initial grid frame.
        let status_val = read_next_ws_json(&mut ws_stream).await;
        assert_eq!(
            status_val.get("type").and_then(|v| v.as_str()),
            Some("remoteStatus"),
            "first frame must be remoteStatus, got: {status_val}"
        );
        let generation = status_val
            .get("generation")
            .and_then(|v| v.as_str())
            .expect("remoteStatus must contain generation")
            .to_string();

        let init_grid_val = read_next_ws_json(&mut ws_stream).await;

        // 5. ASSERTION 1 (initial-geometry fix): initial grid frame has cols == 100 && rows == 30.
        assert_eq!(
            init_grid_val.get("type").and_then(|v| v.as_str()),
            Some("grid"),
            "second frame must be grid, got: {init_grid_val}"
        );
        assert_eq!(
            init_grid_val.get("cols").and_then(|v| v.as_u64()),
            Some(100),
            "initial grid cols must be 100"
        );
        assert_eq!(
            init_grid_val.get("rows").and_then(|v| v.as_u64()),
            Some(30),
            "initial grid rows must be 30"
        );

        // 6. Write b"stty size\n" and read grid frames until concatenated run texts contain "30 100".
        daemon
            .write_session_input(&session, b"stty size\n".to_vec())
            .await
            .unwrap();

        let mut frames_seen_initial: Vec<serde_json::Value> = Vec::new();
        let mut concatenated_runs_initial = String::new();

        let read_initial_stty = tokio::time::timeout(Duration::from_secs(10), async {
            while !concatenated_runs_initial.contains("30 100") {
                let val = read_next_ws_json(&mut ws_stream).await;
                concatenated_runs_initial.push_str(&extract_grid_text(&val));
                frames_seen_initial.push(val);
            }
        })
        .await;
        assert!(
            read_initial_stty.is_ok(),
            "timed out waiting for initial '30 100' grid output; frames seen: {:?}",
            frames_seen_initial
        );

        // 7. Send generation-fenced resize as a Text frame.
        let resize_payload = serde_json::json!({
            "type": "remoteResize",
            "generation": generation,
            "cols": 132,
            "rows": 43,
        });
        tokio::time::timeout(
            Duration::from_secs(10),
            ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(
                resize_payload.to_string().into(),
            )),
        )
        .await
        .expect("timeout sending remoteResize")
        .expect("send remoteResize websocket message");

        // 8. ASSERTION 2 (mirror-resize fix): read grid frames until cols == 132 && rows == 43,
        // then write b"stty size\n" and read grid frames until run texts contain "43 132".
        let mut frames_seen_resize: Vec<serde_json::Value> = Vec::new();
        let read_resize_frame = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let val = read_next_ws_json(&mut ws_stream).await;
                let cols = val.get("cols").and_then(|c| c.as_u64());
                let rows = val.get("rows").and_then(|r| r.as_u64());
                frames_seen_resize.push(val);
                if cols == Some(132) && rows == Some(43) {
                    break;
                }
            }
        })
        .await;
        assert!(
            read_resize_frame.is_ok(),
            "timed out waiting for grid frame with cols=132, rows=43; frames seen: {:?}",
            frames_seen_resize
        );

        daemon
            .write_session_input(&session, b"stty size\n".to_vec())
            .await
            .unwrap();

        let mut frames_seen_resized_stty: Vec<serde_json::Value> = Vec::new();
        let mut concatenated_runs_resized = String::new();

        let read_resized_stty = tokio::time::timeout(Duration::from_secs(10), async {
            while !concatenated_runs_resized.contains("43 132") {
                let val = read_next_ws_json(&mut ws_stream).await;
                concatenated_runs_resized.push_str(&extract_grid_text(&val));
                frames_seen_resized_stty.push(val);
            }
        })
        .await;
        assert!(
            read_resized_stty.is_ok(),
            "timed out waiting for resized '43 132' grid output; frames seen: {:?}",
            frames_seen_resized_stty
        );

        // 9. Print distinct sentinel line on success and cleanup.
        println!("FERRYX_SSH_QA_GRID_RESIZE_OK {session}");

        // 10. Binary input dropped: SSH sessions must ignore WebSocket binary messages.
        let binary_payload = b"printf '\\115\\101\\122\\113\\105\\122\\055\\102\\012'\n".to_vec();
        tokio::time::timeout(
            Duration::from_secs(10),
            ws_stream.send(tokio_tungstenite::tungstenite::Message::Binary(
                binary_payload.into(),
            )),
        )
        .await
        .expect("timeout sending binary message")
        .expect("send binary websocket message");

        let mut frames_seen_binary: Vec<serde_json::Value> = Vec::new();
        let mut concatenated_runs_binary = String::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let val = read_next_ws_json(&mut ws_stream).await;
                concatenated_runs_binary.push_str(&extract_grid_text(&val));
                frames_seen_binary.push(val);
            }
        })
        .await;
        assert!(
            !concatenated_runs_binary.contains("MARKER-B"),
            "binary input must be dropped on SSH session, but MARKER-B appeared in grid text; frames seen: {:?}",
            frames_seen_binary
        );

        // 11. Remote write delivered: remoteWrite text frames must reach the SSH PTY.
        let write_payload = serde_json::json!({
            "type": "remoteWrite",
            "generation": generation,
            "data": "printf '\\115\\101\\122\\113\\105\\122\\055\\101\\012'\n",
        });
        tokio::time::timeout(
            Duration::from_secs(10),
            ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(
                write_payload.to_string().into(),
            )),
        )
        .await
        .expect("timeout sending remoteWrite")
        .expect("send remoteWrite websocket message");

        let mut frames_seen_write: Vec<serde_json::Value> = Vec::new();
        let mut concatenated_runs_write = String::new();

        let read_write_marker = tokio::time::timeout(Duration::from_secs(10), async {
            while !concatenated_runs_write.contains("MARKER-A") {
                let val = read_next_ws_json(&mut ws_stream).await;
                concatenated_runs_write.push_str(&extract_grid_text(&val));
                frames_seen_write.push(val);
            }
        })
        .await;
        assert!(
            read_write_marker.is_ok(),
            "timed out waiting for 'MARKER-A' grid output from remoteWrite; frames seen: {:?}",
            frames_seen_write
        );

        // 12. Print distinct sentinel line on success.
        println!("FERRYX_SSH_QA_INPUT_PATH_OK {session}");
        let _ = tokio::time::timeout(Duration::from_secs(2), ws_stream.close(None)).await;
        server_handle.abort();
        remote_state.clear_active_selection();
        daemon
            .handle_close(&session)
            .await
            .expect("close only QA-owned SSH session");
        result.expect("exact remote CWD through SSH PTY, not input echo");
        println!(
            "FERRYX_SSH_QA_PTY_OK {request} {}",
            config.expected_repo_root
        );
    }
    projects::unregister(&host_store, &response.workspace_id).unwrap();
}
